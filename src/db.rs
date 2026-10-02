use anyhow::{Context, Result};
use sqlx::{PgPool, postgres::PgPoolOptions};

pub async fn connect(url: &str) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(url)
        .await
        .context("connect to Postgres")?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("run migrations")?;
    Ok(pool)
}
