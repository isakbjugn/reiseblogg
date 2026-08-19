//! Databasespørringer.
//!
//! Konvensjon fra fagord-rust-api: funksjoner prefikses `db_`, tar
//! `&PgPool` som siste argument og propagerer `sqlx::Error` – handlerne mapper
//! til HTTP-statuser.

pub mod post;
