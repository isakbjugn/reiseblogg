use sqlx::PgPool;

pub async fn insert(
    db: &PgPool,
    key: &str,
    mime_type: &str,
    author_id: i32,
) -> Result<i32, sqlx::Error> {
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
