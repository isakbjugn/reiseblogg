//! Media-typer: DB-raden galleriet blar i.
//!
//! Samme mønster som `post`: radtypen er en ren DB-rad med kun `Debug + FromRow`
//! og er aldri `Serialize`. Visningstypen templatene ser bygges i handler-filen
//! (`media.rs`), ikke her.

use sqlx::FromRow;

/// Én media-rad, sortert nyeste først i spørringen.
///
/// `key` er objektnavnet i R2 og siste del av den offentlige URL-en. `mime_type`
/// avgjør om galleriet viser `<img>` (bilder) eller `<video>` (mp4).
#[derive(Debug, FromRow)]
pub struct MediaRad {
    pub id: i32,
    pub key: String,
    pub mime_type: String,
}
