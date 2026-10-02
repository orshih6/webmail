//! Per-account data in Postgres: preferences, identities, address book. Every query is
//! scoped by `owner` (the login address) — never trust an id from the client alone.

use std::str::FromStr;

use sqlx::{PgPool, types::Json};

use crate::{
    error::{AppError, AppResult},
    types::{Contact, ContactInput, Identity, IdentityInput, Prefs},
};

const MAX_IDENTITIES: i64 = 20;
const MAX_CONTACTS: i64 = 5000;

pub async fn prefs(db: &PgPool, owner: &str) -> AppResult<Prefs> {
    let row: Option<(Json<Prefs>,)> = sqlx::query_as("SELECT data FROM prefs WHERE owner = $1")
        .bind(owner)
        .fetch_optional(db)
        .await?;
    Ok(row.map(|r| r.0.0).unwrap_or_default())
}

pub async fn save_prefs(db: &PgPool, owner: &str, p: &Prefs) -> AppResult<Prefs> {
    let mut p = p.clone();
    p.page_size = p.page_size.clamp(10, 200);
    sqlx::query(
        "INSERT INTO prefs (owner, data) VALUES ($1, $2)
         ON CONFLICT (owner) DO UPDATE SET data = EXCLUDED.data",
    )
    .bind(owner)
    .bind(Json(&p))
    .execute(db)
    .await?;
    Ok(p)
}

type IdentityRow = (i64, String, String, String, bool);

fn identity(owner: &str, r: IdentityRow) -> Identity {
    Identity {
        id: Some(r.0),
        email: owner.to_owned(),
        name: r.1,
        reply_to: r.2,
        signature: r.3,
        is_default: r.4,
    }
}

/// The account's identities, default first. Never empty: an account that saved none gets
/// a built-in one (no name, no signature).
pub async fn identities(db: &PgPool, owner: &str) -> AppResult<Vec<Identity>> {
    let rows: Vec<IdentityRow> = sqlx::query_as(
        "SELECT id, name, reply_to, signature, is_default FROM identities
         WHERE owner = $1 ORDER BY is_default DESC, id",
    )
    .bind(owner)
    .fetch_all(db)
    .await?;
    if rows.is_empty() {
        return Ok(vec![Identity {
            id: None,
            email: owner.to_owned(),
            name: String::new(),
            reply_to: String::new(),
            signature: String::new(),
            is_default: true,
        }]);
    }
    Ok(rows.into_iter().map(|r| identity(owner, r)).collect())
}

/// The identity to send with: the requested one if it is the owner's, else the default.
pub async fn identity_for_send(db: &PgPool, owner: &str, id: Option<i64>) -> AppResult<Identity> {
    let all = identities(db, owner).await?;
    let chosen = id.and_then(|id| all.iter().find(|i| i.id == Some(id)));
    Ok(chosen.unwrap_or(&all[0]).clone())
}

fn clean_line(s: &str, field: &str, max: usize) -> AppResult<String> {
    let s = s.trim();
    if s.len() > max || s.contains(['\r', '\n']) {
        return Err(AppError::BadRequest(format!("{field} is invalid")));
    }
    Ok(s.to_owned())
}

pub async fn save_identity(
    db: &PgPool,
    owner: &str,
    i: &IdentityInput,
) -> AppResult<Vec<Identity>> {
    let name = clean_line(&i.name, "name", 200)?;
    let reply_to = clean_line(&i.reply_to, "Reply-To", 254)?;
    if !reply_to.is_empty() {
        lettre::Address::from_str(&reply_to)
            .map_err(|_| AppError::BadRequest("Reply-To is not an email address".into()))?;
    }
    if i.signature.len() > 10_000 {
        return Err(AppError::BadRequest("signature is too long".into()));
    }
    let signature = i.signature.replace("\r\n", "\n").trim_end().to_owned();

    let mut tx = db.begin().await?;
    let count: (i64,) = sqlx::query_as("SELECT count(*) FROM identities WHERE owner = $1")
        .bind(owner)
        .fetch_one(&mut *tx)
        .await?;
    // The first saved identity is the default whatever the client says.
    let make_default = i.is_default || count.0 == 0;
    if make_default {
        sqlx::query("UPDATE identities SET is_default = false WHERE owner = $1")
            .bind(owner)
            .execute(&mut *tx)
            .await?;
    }
    match i.id {
        Some(id) => {
            let r = sqlx::query(
                "UPDATE identities SET name = $3, reply_to = $4, signature = $5,
                        is_default = is_default OR $6
                 WHERE id = $1 AND owner = $2",
            )
            .bind(id)
            .bind(owner)
            .bind(&name)
            .bind(&reply_to)
            .bind(&signature)
            .bind(make_default)
            .execute(&mut *tx)
            .await?;
            if r.rows_affected() == 0 {
                return Err(AppError::NotFound);
            }
        }
        None => {
            if count.0 >= MAX_IDENTITIES {
                return Err(AppError::BadRequest("too many identities".into()));
            }
            sqlx::query(
                "INSERT INTO identities (owner, name, reply_to, signature, is_default)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(owner)
            .bind(&name)
            .bind(&reply_to)
            .bind(&signature)
            .bind(make_default)
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    identities(db, owner).await
}

pub async fn delete_identity(db: &PgPool, owner: &str, id: i64) -> AppResult<Vec<Identity>> {
    let mut tx = db.begin().await?;
    let was_default: Option<(bool,)> =
        sqlx::query_as("DELETE FROM identities WHERE id = $1 AND owner = $2 RETURNING is_default")
            .bind(id)
            .bind(owner)
            .fetch_optional(&mut *tx)
            .await?;
    if was_default.is_some_and(|d| d.0) {
        sqlx::query(
            "UPDATE identities SET is_default = true
             WHERE id = (SELECT min(id) FROM identities WHERE owner = $1)",
        )
        .bind(owner)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    identities(db, owner).await
}

type ContactRow = (i64, String, String, bool);

fn contact(r: ContactRow) -> Contact {
    Contact {
        id: r.0,
        name: r.1,
        email: r.2,
        saved: r.3,
    }
}

/// Autocomplete when `q` is set (10 best, most used first); otherwise the saved address
/// book, alphabetically.
pub async fn contacts(db: &PgPool, owner: &str, q: Option<&str>) -> AppResult<Vec<Contact>> {
    let rows: Vec<ContactRow> = match q.map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => {
            let pat = format!("%{}%", q.replace(['\\', '%', '_'], ""));
            sqlx::query_as(
                "SELECT id, name, email, saved FROM contacts
                 WHERE owner = $1 AND (email ILIKE $2 OR name ILIKE $2)
                 ORDER BY saved DESC, use_count DESC, last_used_at DESC NULLS LAST
                 LIMIT 10",
            )
            .bind(owner)
            .bind(pat)
            .fetch_all(db)
            .await?
        }
        None => {
            sqlx::query_as(
                "SELECT id, name, email, saved FROM contacts WHERE owner = $1 AND saved
                 ORDER BY lower(coalesce(nullif(name, ''), email))",
            )
            .bind(owner)
            .fetch_all(db)
            .await?
        }
    };
    Ok(rows.into_iter().map(contact).collect())
}

pub async fn save_contact(db: &PgPool, owner: &str, c: &ContactInput) -> AppResult<Contact> {
    let name = clean_line(&c.name, "name", 200)?;
    let email = clean_line(&c.email, "email", 254)?;
    lettre::Address::from_str(&email)
        .map_err(|_| AppError::BadRequest("not an email address".into()))?;

    let row: Option<ContactRow> = match c.id {
        Some(id) => sqlx::query_as(
            "UPDATE contacts SET name = $3, email = $4, saved = true
                 WHERE id = $1 AND owner = $2 RETURNING id, name, email, saved",
        )
        .bind(id)
        .bind(owner)
        .bind(&name)
        .bind(&email)
        .fetch_optional(db)
        .await
        .map_err(unique_violation)?,
        None => {
            let n: (i64,) = sqlx::query_as("SELECT count(*) FROM contacts WHERE owner = $1")
                .bind(owner)
                .fetch_one(db)
                .await?;
            if n.0 >= MAX_CONTACTS {
                return Err(AppError::BadRequest("address book is full".into()));
            }
            // Saving an address that was only collected promotes it.
            sqlx::query_as(
                "INSERT INTO contacts (owner, name, email, saved) VALUES ($1, $2, $3, true)
                 ON CONFLICT (owner, lower(email)) DO UPDATE
                   SET name = EXCLUDED.name, saved = true
                 RETURNING id, name, email, saved",
            )
            .bind(owner)
            .bind(&name)
            .bind(&email)
            .fetch_optional(db)
            .await?
        }
    };
    row.map(contact).ok_or(AppError::NotFound)
}

fn unique_violation(e: sqlx::Error) -> AppError {
    match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => {
            AppError::BadRequest("that address is already in your contacts".into())
        }
        _ => e.into(),
    }
}

pub async fn delete_contact(db: &PgPool, owner: &str, id: i64) -> AppResult<()> {
    sqlx::query("DELETE FROM contacts WHERE id = $1 AND owner = $2")
        .bind(id)
        .bind(owner)
        .execute(db)
        .await?;
    Ok(())
}

/// Remembers who the user writes to, for autocomplete. Best effort: never fails a send.
pub async fn collect_recipients(db: &PgPool, owner: &str, rcpts: &[(String, String)]) {
    for (name, email) in rcpts.iter().take(100) {
        let r = sqlx::query(
            "INSERT INTO contacts (owner, name, email, use_count, last_used_at)
             VALUES ($1, $2, $3, 1, now())
             ON CONFLICT (owner, lower(email)) DO UPDATE
               SET use_count = contacts.use_count + 1, last_used_at = now(),
                   name = CASE WHEN contacts.name = '' THEN EXCLUDED.name ELSE contacts.name END",
        )
        .bind(owner)
        .bind(name)
        .bind(email)
        .execute(db)
        .await;
        if let Err(e) = r {
            tracing::warn!(error = %e, "collect recipient");
            return;
        }
    }
}

pub async fn is_contact(db: &PgPool, owner: &str, email: &str) -> AppResult<bool> {
    let r: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM contacts WHERE owner = $1 AND lower(email) = lower($2) AND saved",
    )
    .bind(owner)
    .bind(email)
    .fetch_optional(db)
    .await?;
    Ok(r.is_some())
}
