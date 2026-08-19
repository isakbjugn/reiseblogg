//! Post-spørringer. Mønster fra fagord-rust-api (`db/article.rs`):
//! `query_as!`-makroer som valideres mot databasen ved kompilering, og
//! `sqlx::Error` propagert opp til handlerne.
//!
//! `AS "published_at!"` – `!`-suffikset forteller `query_as!` at kolonnen er
//! ikke-null, selv om `published_at` er nullable i skjemaet. De to publiserte
//! spørringene har `WHERE p.published_at IS NOT NULL`, så det stemmer.

use sqlx::PgPool;

use crate::types::post::{PostRad, PostRedigerRad};

/// Publiserte poster, nyeste først. Forsiden og arkivet deler denne.
pub async fn db_get_poster(db: &PgPool) -> Result<Vec<PostRad>, sqlx::Error> {
    sqlx::query_as!(
        PostRad,
        r#"
        SELECT p.slug, p.title, p.content, p.published_at AS "published_at!", a.name AS forfatter
        FROM post p
        JOIN author a ON a.id = p.created_by
        WHERE p.published_at IS NOT NULL
        ORDER BY p.published_at DESC
        "#,
    )
    .fetch_all(db)
    .await
}

/// Én publisert post. En kladd matcher ikke WHERE-en og gir `RowNotFound`,
/// som handleren mapper til 404 – kladder skal ikke kunne leses offentlig.
pub async fn db_get_publisert_post(slug: &str, db: &PgPool) -> Result<PostRad, sqlx::Error> {
    sqlx::query_as!(
        PostRad,
        r#"
        SELECT p.slug, p.title, p.content, p.published_at AS "published_at!", a.name AS forfatter
        FROM post p
        JOIN author a ON a.id = p.created_by
        WHERE p.slug = $1 AND p.published_at IS NOT NULL
        "#,
        slug,
    )
    .fetch_one(db)
    .await
}

/// Én post uansett publiseringsstatus, til editoren.
pub async fn db_get_post(slug: &str, db: &PgPool) -> Result<PostRedigerRad, sqlx::Error> {
    sqlx::query_as!(
        PostRedigerRad,
        r#"
        SELECT slug, title, content
        FROM post
        WHERE slug = $1
        "#,
        slug,
    )
    .fetch_one(db)
    .await
}
