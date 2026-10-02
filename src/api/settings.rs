use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::{
    AppState,
    error::AppResult,
    session::Auth,
    settings,
    types::{Contact, ContactInput, Identity, IdentityInput, Prefs},
};

pub async fn get_prefs(State(st): State<AppState>, auth: Auth) -> AppResult<Json<Prefs>> {
    Ok(Json(settings::prefs(&st.db, &auth.email).await?))
}

pub async fn set_prefs(
    State(st): State<AppState>,
    auth: Auth,
    Json(p): Json<Prefs>,
) -> AppResult<Json<Prefs>> {
    Ok(Json(settings::save_prefs(&st.db, &auth.email, &p).await?))
}

pub async fn identities(State(st): State<AppState>, auth: Auth) -> AppResult<Json<Vec<Identity>>> {
    Ok(Json(settings::identities(&st.db, &auth.email).await?))
}

pub async fn save_identity(
    State(st): State<AppState>,
    auth: Auth,
    Json(i): Json<IdentityInput>,
) -> AppResult<Json<Vec<Identity>>> {
    Ok(Json(
        settings::save_identity(&st.db, &auth.email, &i).await?,
    ))
}

pub async fn delete_identity(
    State(st): State<AppState>,
    auth: Auth,
    Path(id): Path<i64>,
) -> AppResult<Json<Vec<Identity>>> {
    Ok(Json(
        settings::delete_identity(&st.db, &auth.email, id).await?,
    ))
}

#[derive(Deserialize)]
pub struct ContactQuery {
    q: Option<String>,
}

pub async fn contacts(
    State(st): State<AppState>,
    auth: Auth,
    Query(q): Query<ContactQuery>,
) -> AppResult<Json<Vec<Contact>>> {
    Ok(Json(
        settings::contacts(&st.db, &auth.email, q.q.as_deref()).await?,
    ))
}

pub async fn save_contact(
    State(st): State<AppState>,
    auth: Auth,
    Json(c): Json<ContactInput>,
) -> AppResult<Json<Contact>> {
    Ok(Json(settings::save_contact(&st.db, &auth.email, &c).await?))
}

pub async fn delete_contact(
    State(st): State<AppState>,
    auth: Auth,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    settings::delete_contact(&st.db, &auth.email, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
