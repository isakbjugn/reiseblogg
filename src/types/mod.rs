//! Typer som deles på tvers av handlerne.
//!
//! Mønster fra fagord-rust-api: row-typene i `post` har kun `Debug + FromRow`
//! og er aldri `Serialize` – visningstypene templatene ser bygges i
//! handler-filene.

pub mod post;
