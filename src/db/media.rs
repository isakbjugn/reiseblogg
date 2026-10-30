use sqlx::PgPool;

use crate::types::media::MediaRad;

pub async fn insert(db: &PgPool, key: &str, mime_type: &str, author_id: i32) -> Result<i32, sqlx::Error> {
    let media_id = sqlx::query_scalar::<_, i32>(
        "INSERT INTO media (key, mime_type, uploaded_by) \
            VALUES ($1, $2, $3) \
            RETURNING id",
    )
    .bind(key)
    .bind(mime_type)
    .bind(author_id)
    .fetch_one(db)
    .await?;

    Ok(media_id)
}

/// Alle media, nyeste først. Galleriet henter hele listen for å vite hvem som er
/// «forrige» og «neste» uten to ekstra spørringer per sidevisning. For en blogg
/// med noen få hundre bilder er det trivielt; `LAG`/`LEAD`-spørringer er en
/// senere optimalisering om det noen gang blir nødvendig.
pub async fn db_liste_media(db: &PgPool) -> Result<Vec<MediaRad>, sqlx::Error> {
    sqlx::query_as!(
        MediaRad,
        r#"
        SELECT id, key, mime_type
        FROM media
        ORDER BY created_at DESC, id DESC
        "#,
    )
    .fetch_all(db)
    .await
}
