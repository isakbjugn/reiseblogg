//! Post-spørringer. Mønster fra fagord-rust-api (`db/article.rs`):
//! `query_as!`-makroer som valideres mot databasen ved kompilering, og
//! `sqlx::Error` propagert opp til handlerne.
//!
//! `AS "published_at!"` – `!`-suffikset forteller `query_as!` at kolonnen er
//! ikke-null, selv om `published_at` er nullable i skjemaet. De to publiserte
//! spørringene har `WHERE p.published_at IS NOT NULL`, så det stemmer.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::types::post::{KladdRad, PostRad, PostRedigerRad};

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
        SELECT slug, title, content, published_at
        FROM post
        WHERE slug = $1
        "#,
        slug,
    )
    .fetch_one(db)
    .await
}

/// Alle kladder, nyest endret først. Bevisst *uten* `created_by`-filter:
/// autorisasjonen i steg 4 er flat (enhver innlogget forfatter kan endre og
/// slette alt), så en avpublisert post skal forbli synlig og redigerbar for alle
/// – uavhengig av hvem som opprettet den. En kladd som bare eieren så, ville
/// «forsvinne» i det en annen forfatter avpubliserte den.
pub async fn db_get_kladder(db: &PgPool) -> Result<Vec<KladdRad>, sqlx::Error> {
    sqlx::query_as!(
        KladdRad,
        r#"
        SELECT slug, title, updated_at
        FROM post
        WHERE published_at IS NULL
        ORDER BY updated_at DESC
        "#,
    )
    .fetch_all(db)
    .await
}

/// Oppretter en post. Slugen er generert og unik-kontrollert i handleren før
/// kallet; databasens `UNIQUE`-begrensning er bare siste skanse. `updated_by`
/// forblir `NULL` på en ny post (jf. migrasjonens kommentar).
///
/// `publisert` styrer `published_at`: `Some(now())` gjør posten synlig med en
/// gang, `None` gjør den til kladd. Klokka settes i Rust (ikke `now()` i SQL)
/// for å holde verdien i sync med det vi eventuelt viser etterpå.
pub async fn db_opprett_post(
    slug: &str,
    title: &str,
    content: &str,
    author_id: i32,
    publisert: bool,
    db: &PgPool,
) -> Result<(), sqlx::Error> {
    let published_at = publisert.then(Utc::now);

    sqlx::query!(
        r#"
        INSERT INTO post (slug, title, content, created_by, published_at)
        VALUES ($1, $2, $3, $4, $5)
        "#,
        slug,
        title,
        content,
        author_id,
        published_at,
    )
    .execute(db)
    .await?;

    Ok(())
}

/// Lagrer tittel og innhold, uten å røre `published_at` – «Lagre»-handlingen
/// beholder publiseringsstatusen. `RETURNING slug` gjør at en ukjent slug gir
/// `RowNotFound`, som handleren mapper til 404.
pub async fn db_oppdater_innhold(
    slug: &str,
    title: &str,
    content: &str,
    author_id: i32,
    db: &PgPool,
) -> Result<String, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        UPDATE post SET title = $2, content = $3, updated_by = $4
        WHERE slug = $1
        RETURNING slug
        "#,
        slug,
        title,
        content,
        author_id,
    )
    .fetch_one(db)
    .await
}

/// Publiserer eller avpubliserer: `published_at = now()` ved `Some(now())`,
/// `NULL` ved `None`. `Some`/`None` beregnes av handleren fra «publiser»/
/// «avpubliser»-knappen.
pub async fn db_sett_publisert(
    slug: &str,
    published_at: Option<DateTime<Utc>>,
    author_id: i32,
    db: &PgPool,
) -> Result<String, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        UPDATE post SET published_at = $2, updated_by = $3
        WHERE slug = $1
        RETURNING slug
        "#,
        slug,
        published_at,
        author_id,
    )
    .fetch_one(db)
    .await
}

/// Sletter en post. `RETURNING slug` gir `RowNotFound` hvis slugen ikke finnes,
/// så handleren kan svare 404 framfor å late som noe ble slettet.
pub async fn db_slett_post(slug: &str, db: &PgPool) -> Result<String, sqlx::Error> {
    sqlx::query_scalar!("DELETE FROM post WHERE slug = $1 RETURNING slug", slug)
        .fetch_one(db)
        .await
}

/// Sant hvis slugen alt er i bruk. Brukes av slug-genereringen for å legge på
/// et `-2`/`-3`-suffiks framfor å kollidere med `UNIQUE`-begrensningen.
pub async fn db_slug_finnes(slug: &str, db: &PgPool) -> Result<bool, sqlx::Error> {
    // `EXISTS` gir alltid `true`/`false`, men `query_scalar!` rapporterer
    // kolonnen som nullable – `unwrap_or` er kun for typeoppsettet.
    let finnes: Option<bool> = sqlx::query_scalar!("SELECT EXISTS(SELECT 1 FROM post WHERE slug = $1)", slug)
        .fetch_one(db)
        .await?;
    Ok(finnes.unwrap_or(false))
}
