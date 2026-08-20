//! Poster: handlere for lesesidene og editoren.
//!
//! Postene leses fra `post`-tabellen via `crate::db::post`. Skriving og
//! publisering kommer i steg 4 (se PLAN.md).

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Html;
use minijinja::context;
use sqlx::Error;

use crate::db::post::{db_get_kladder, db_get_post, db_get_poster, db_get_publisert_post};
use crate::extract::AuthenticatedAuthor;
use crate::types::post::{PostRad, PostVisning};
use crate::{AppState, markdown, rendre};

/// Konverterer en DB-rad til visningstypen templatene ser.
///
/// Konverteringen ligger her, i handler-filen, som i fagord (`articles.rs`) –
/// `types/` holder bare selve typene. `created_by`/`updated_by` ble aldri
/// selekt-ert, så radene har ingenting som kan lekke ut.
impl PostVisning {
    fn fra_rad(rad: PostRad) -> Self {
        // Bevisst UTC: datoen brukes bare til visning, og `published_at` settes
        // med `now()` ved publisering (steg 4). Om datodriften irriterer, er
        // `chrono-tz` (Europe/Oslo) den naturlige løsningen.
        let dato = rad.published_at.format("%Y-%m-%d").to_string();
        Self {
            slug: rad.slug,
            tittel: rad.title,
            dato_lesbar: norsk_dato(&dato),
            dato,
            forfatter: rad.forfatter,
            sammendrag: markdown::sammendrag(&rad.content, 180),
            innhold_html: markdown::til_html(&rad.content),
        }
    }
}

/// SQLx-feil som ikke er «raden finnes ikke»: logg og svar 500.
fn db_feil(e: Error) -> StatusCode {
    tracing::error!("Databasefeil: {e:#?}");
    StatusCode::INTERNAL_SERVER_ERROR
}

/// Norske månedsnavn. Brukes til lesbare datoer og arkivgruppering.
///
/// Egen funksjon framfor et lokaliseringsbibliotek: tolv strenger på ett språk er
/// ikke et bibliotekbehov.
fn maned_navn(maned: u32) -> &'static str {
    match maned {
        1 => "januar",
        2 => "februar",
        3 => "mars",
        4 => "april",
        5 => "mai",
        6 => "juni",
        7 => "juli",
        8 => "august",
        9 => "september",
        10 => "oktober",
        11 => "november",
        12 => "desember",
        _ => "ukjent",
    }
}

/// Deler `YYYY-MM-DD` i (år, måned, dag). `None` hvis formen er feil.
fn del_dato(iso: &str) -> Option<(i32, u32, u32)> {
    let mut deler = iso.split('-');
    let år = deler.next()?.parse().ok()?;
    let måned = deler.next()?.parse().ok()?;
    let dag = deler.next()?.parse().ok()?;
    Some((år, måned, dag))
}

/// `2026-08-02` → `2. august 2026`. Faller tilbake til ISO-strengen ved rar input.
fn norsk_dato(iso: &str) -> String {
    match del_dato(iso) {
        Some((år, måned, dag)) => format!("{}. {} {}", dag, maned_navn(måned), år),
        None => iso.to_string(),
    }
}

/// Forsiden: postliste med sammendrag.
pub async fn forside(
    State(state): State<Arc<AppState>>,
    author: Option<AuthenticatedAuthor>,
) -> Result<Html<String>, StatusCode> {
    let poster: Vec<_> = db_get_poster(&state.db)
        .await
        .map_err(db_feil)?
        .into_iter()
        .map(PostVisning::fra_rad)
        .map(|p| {
            context! {
                slug => p.slug,
                tittel => p.tittel,
                dato => p.dato,
                dato_lesbar => p.dato_lesbar,
                forfatter => p.forfatter,
                sammendrag => p.sammendrag,
            }
        })
        .collect();

    // Kun egne kladder – `db_get_kladder` filtrerer på `created_by`.
    let kladder: Vec<_> = match &author {
        Some(a) => db_get_kladder(a.id, &state.db)
            .await
            .map_err(db_feil)?
            .into_iter()
            .map(|k| {
                let dato = k.updated_at.format("%Y-%m-%d").to_string();
                context! {
                    slug => k.slug,
                    tittel => k.title,
                    sist_endret => norsk_dato(&dato),
                }
            })
            .collect(),
        None => Vec::new(),
    };

    rendre(
        &state,
        "forside.html",
        context! {
            poster,
            kladder,
            paalogget => author.is_some(),
        },
    )
}

/// En enkelt post. Markdown rendres her, ikke i templaten.
pub async fn vis_post(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
    author: Option<AuthenticatedAuthor>,
) -> Result<Html<String>, StatusCode> {
    // `RowNotFound` dekker både ukjent slug og kladd – begge er 404 på lesesiden,
    // så en kladd-slug avslører ikke at posten finnes.
    let rad = db_get_publisert_post(&slug, &state.db).await.map_err(|e| match e {
        Error::RowNotFound => StatusCode::NOT_FOUND,
        e => db_feil(e),
    })?;
    let post = PostVisning::fra_rad(rad);

    rendre(
        &state,
        "post.html",
        context! {
            slug => post.slug,
            tittel => post.tittel,
            dato => post.dato,
            dato_lesbar => post.dato_lesbar,
            forfatter => post.forfatter,
            innhold => post.innhold_html,
            // Til <meta description> og Open Graph.
            sammendrag => post.sammendrag,
            paalogget => author.is_some(),
        },
    )
}

/// Arkiv: alle poster gruppert på år og måned.
pub async fn arkiv(
    State(state): State<Arc<AppState>>,
    author: Option<AuthenticatedAuthor>,
) -> Result<Html<String>, StatusCode> {
    // db_get_poster er allerede sortert nyeste først, så vi kan gruppere
    // sekvensielt og slipper å sortere gruppene etterpå.
    let mut grupper: Vec<minijinja::Value> = Vec::new();
    let mut nåværende: Option<(i32, u32, Vec<minijinja::Value>)> = None;

    for post in db_get_poster(&state.db)
        .await
        .map_err(db_feil)?
        .into_iter()
        .map(PostVisning::fra_rad)
    {
        let (år, måned, _) = del_dato(&post.dato).unwrap_or((0, 0, 0));

        let oppføring = context! {
            slug => post.slug,
            tittel => post.tittel,
            dato => post.dato,
            dato_lesbar => post.dato_lesbar,
            forfatter => post.forfatter,
        };

        match &mut nåværende {
            Some((g_år, g_måned, poster)) if *g_år == år && *g_måned == måned => {
                poster.push(oppføring);
            }
            _ => {
                if let Some((g_år, g_måned, poster)) = nåværende.take() {
                    grupper.push(context! {
                        tittel => format!("{} {}", maned_navn(g_måned), g_år),
                        poster => poster,
                    });
                }
                nåværende = Some((år, måned, vec![oppføring]));
            }
        }
    }

    if let Some((g_år, g_måned, poster)) = nåværende {
        grupper.push(context! {
            tittel => format!("{} {}", maned_navn(g_måned), g_år),
            poster => poster,
        });
    }

    rendre(
        &state,
        "arkiv.html",
        context! { grupper, paalogget => author.is_some() },
    )
}

/// Om oss – statisk side.
pub async fn om_oss(
    State(state): State<Arc<AppState>>,
    author: Option<AuthenticatedAuthor>,
) -> Result<Html<String>, StatusCode> {
    rendre(&state, "om-oss.html", context! { paalogget => author.is_some() })
}

const NY_POST_MAL: &str = "Skriv her.\n\n\
                           Forhåndsvisningen til høyre oppdateres mens du skriver.\n";

/// Ny post: tom editor.
pub async fn ny_post(
    State(state): State<Arc<AppState>>,
    author: Option<AuthenticatedAuthor>,
) -> Result<Html<String>, StatusCode> {
    rendre(
        &state,
        "editor.html",
        context! {
            overskrift => "Ny post",
            handling => "/ny",
            avbryt => "/",
            slug => "",
            tittel => "",
            innhold => NY_POST_MAL,
            paalogget => author.is_some(),
        },
    )
}

/// Rediger eksisterende post: samme editor, forhåndsfylt.
///
/// Lagring kommer i steg 4 – handleren viser bare skjemaet nå.
pub async fn rediger_post(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
    author: Option<AuthenticatedAuthor>,
) -> Result<Html<String>, StatusCode> {
    let rad = db_get_post(&slug, &state.db).await.map_err(|e| match e {
        Error::RowNotFound => StatusCode::NOT_FOUND,
        e => db_feil(e),
    })?;

    rendre(
        &state,
        "editor.html",
        context! {
            overskrift => format!("Rediger «{}»", rad.title),
            handling => format!("/rediger/{}", rad.slug),
            avbryt => format!("/post/{}", rad.slug),
            slug => rad.slug,
            tittel => rad.title,
            innhold => rad.content,
            paalogget => author.is_some(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norsk_dato_formaterer() {
        assert_eq!(norsk_dato("2026-08-02"), "2. august 2026");
        assert_eq!(norsk_dato("2026-12-24"), "24. desember 2026");
    }

    #[test]
    fn norsk_dato_faller_tilbake_ved_rar_input() {
        assert_eq!(norsk_dato("tull"), "tull");
        assert_eq!(norsk_dato("2026-13-01"), "1. ukjent 2026");
    }

    #[test]
    fn del_dato_avviser_ufullstendig() {
        assert!(del_dato("2026-08").is_none());
        assert!(del_dato("").is_none());
    }
}
