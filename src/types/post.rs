//! Post-typer: DB-rader og visningstypen templatene ser.
//!
//! Row-typene (`PostRad`, `PostRedigerRad`) er rene DB-rader med kun
//! `Debug + FromRow` – aldri `Serialize`. Spørringene selekt-er aldri
//! `created_by`/`updated_by`, så interne id-er kan ikke lekke ut i templatene.
//! `PostVisning` er den eneste typen handlerne sender videre.

use chrono::{DateTime, Utc};
use sqlx::FromRow;

/// Publisert post med forfatternavn. `AS forfatter`-aliaset i spørringen matcher
/// feltnavnet her, som igjen matcher template-variablene.
#[derive(Debug, FromRow)]
pub struct PostRad {
    pub slug: String,
    pub title: String,
    /// Markdown, ikke HTML. Rendres av `markdown::til_html` ved visning.
    pub content: String,
    pub published_at: DateTime<Utc>,
    pub forfatter: String,
}

/// Post til editoren – slug, tittel og Markdown. Kladd eller publisert;
/// editoren trenger ingen publiseringsdato.
#[derive(Debug, FromRow)]
pub struct PostRedigerRad {
    pub slug: String,
    pub title: String,
    pub content: String,
}

/// Visningstypen templatene ser. Bygges fra `PostRad` i `poster.rs` – fagord
/// gjør konverteringen i handler-filen, ikke i types/.
pub struct PostVisning {
    pub slug: String,
    pub tittel: String,
    /// ISO-dato (`YYYY-MM-DD`), som `<time datetime>`-verdien.
    pub dato: String,
    pub dato_lesbar: String,
    pub forfatter: String,
    pub sammendrag: String,
    /// Ferdig rendret HTML fra `markdown::til_html` – templaten bruker `| safe`.
    pub innhold_html: String,
}
