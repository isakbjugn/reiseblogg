//! Sesjoner: lagring og oppslag. Mønsteret er hentet fra fagord-rust-api
//! (`db/session.rs`), men tabellen heter `author` her, ikke `contributor`.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// En nyutstedt sesjon, slik auth-flyten trenger den tilbake etter utstedelse.
/// Selve tokenet er *ikke* med her – det finnes kun i klartekst der det genereres,
/// og det vi lagrer er hashen. `expires_at` regnes av databasen (klokke-fasit).
pub struct NewSession {
    pub id: i32,
    pub expires_at: DateTime<Utc>,
}

/// Oppretter en sesjon for en forfatter og returnerer id og utløpstid.
/// Vi lagrer kun `token_hash` (SHA-256 av sesjonstokenet), aldri klartekst, slik
/// at en databaselekkasje ikke gir kaprede sesjoner. Levetiden (30 dager) regnes
/// i SQL med `now()`, så databasen er eneste tidskilde – likt magic_tokens.
pub async fn db_create_session(author_id: i32, token_hash: &str, db: &PgPool) -> Result<NewSession, sqlx::Error> {
    sqlx::query_as!(
        NewSession,
        "INSERT INTO session (author_id, token_hash, expires_at)
         VALUES ($1, $2, now() + interval '30 days')
         RETURNING id, expires_at",
        author_id,
        token_hash
    )
    .fetch_one(db)
    .await
}

/// Forfatteren bak en gyldig sesjon. `role` følger med så autorisasjon (eier
/// ELLER admin) kan avgjøres uten et nytt oppslag.
pub struct SessionAuthor {
    pub id: i32,
    pub role: String,
}

/// Slår opp forfatteren bak et sesjonstoken, men kun hvis sesjonen ikke er
/// utløpt (`expires_at > now()`, regnet i databasen). `None` ved ukjent eller
/// utløpt token. Dette er oppslaget den autentiserende extractoren bygger på.
pub async fn db_find_session_author(token_hash: &str, db: &PgPool) -> Result<Option<SessionAuthor>, sqlx::Error> {
    sqlx::query_as!(
        SessionAuthor,
        "SELECT a.id, a.role
         FROM session s
         JOIN author a ON a.id = s.author_id
         WHERE s.token_hash = $1 AND s.expires_at > now()",
        token_hash
    )
    .fetch_optional(db)
    .await
}

/// Sletter sesjonen som matcher token-hashen (utlogging). Returnerer antall
/// slettede rader; `0` hvis ingen matchet (allerede utlogget/utløpt).
pub async fn db_delete_session(token_hash: &str, db: &PgPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!("DELETE FROM session WHERE token_hash = $1", token_hash)
        .execute(db)
        .await?;

    Ok(result.rows_affected())
}
