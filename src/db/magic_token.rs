//! Engangskoder: lagring og verifisering. Mønsteret er hentet fra fagord-rust-api
//! (`db/magic_token.rs`), men tabellen heter `author` her, ikke `contributor`.

use sqlx::PgPool;

/// En forfatter slik auth-flyten trenger den – her holder `id` (e-posten brukes
/// kun til oppslaget).
pub struct Author {
    pub id: i32,
}

/// En gjeldende (ubrukt, ikke utløpt) engangskode. Forsøkstelleren leses bevisst
/// ikke hit – grensen håndheves atomisk i `db_register_attempt`.
pub struct ActiveToken {
    pub id: i32,
    pub token_hash: String,
}

/// Slår opp en forfatter på e-post, case-insensitivt (likt den unike
/// `lower(email)`-indeksen). `None` hvis e-posten ikke finnes.
pub async fn db_find_author_by_email(email: &str, db: &PgPool) -> Result<Option<Author>, sqlx::Error> {
    sqlx::query_as!(Author, "SELECT id FROM author WHERE lower(email) = lower($1)", email)
        .fetch_optional(db)
        .await
}

/// Utsteder en ny engangskode og sletter samtidig tidligere ubrukte koder for
/// brukeren. Begge steg i én transaksjon, så vi aldri ender med to gyldige koder
/// (eller null hvis innsettingen feiler). Utløpstiden regnes i databasen.
pub async fn db_replace_magic_token(author_id: i32, token_hash: &str, db: &PgPool) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;

    sqlx::query!(
        "DELETE FROM magic_tokens WHERE author_id = $1 AND used_at IS NULL",
        author_id
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!(
        "INSERT INTO magic_tokens (author_id, token_hash, expires_at)
         VALUES ($1, $2, now() + interval '15 minutes')",
        author_id,
        token_hash
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await
}

/// Henter den gjeldende koden for en forfatter: ubrukt og ikke utløpt. Det skal
/// normalt finnes høyst én, men vi sorterer på nyeste for å være robuste.
pub async fn db_find_active_token(author_id: i32, db: &PgPool) -> Result<Option<ActiveToken>, sqlx::Error> {
    sqlx::query_as!(
        ActiveToken,
        "SELECT id, token_hash
         FROM magic_tokens
         WHERE author_id = $1 AND used_at IS NULL AND expires_at > now()
         ORDER BY created_at DESC
         LIMIT 1",
        author_id
    )
    .fetch_optional(db)
    .await
}

/// Reserverer ett verifiseringsforsøk og håndhever grensen i samme `UPDATE ...
/// WHERE attempts < $2`. Returnerer `Some(nytt_antall)`, eller `None` hvis grensen
/// alt var nådd.
///
/// At sjekk og opptelling skjer i én setning er nøkkelen: Postgres tar radlås på
/// `UPDATE`, så samtidige gjettinger kan ikke alle lese «under grensen» og slippe
/// forbi slik et separat les-så-skriv ville tillatt.
pub async fn db_register_attempt(token_id: i32, max_attempts: i32, db: &PgPool) -> Result<Option<i32>, sqlx::Error> {
    let record = sqlx::query!(
        "UPDATE magic_tokens SET attempts = attempts + 1
         WHERE id = $1 AND attempts < $2
         RETURNING attempts",
        token_id,
        max_attempts
    )
    .fetch_optional(db)
    .await?;

    Ok(record.map(|r| r.attempts))
}

/// Markerer en kode som forbrukt (engangsbruk) ved vellykket verifisering, så den
/// ikke kan brukes to ganger. Forsøksgrensen låser koden separat, i `db_register_attempt`.
pub async fn db_mark_token_used(token_id: i32, db: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query!("UPDATE magic_tokens SET used_at = now() WHERE id = $1", token_id)
        .execute(db)
        .await?;

    Ok(())
}
