//! Poster: handlere for lesesidene, editoren og skriving (opprett, rediger,
//! publiser/avpubliser, slett).
//!
//! Postene leses og skrives via `crate::db::post`. Autorisasjonen er flat:
//! enhver innlogget forfatter kan endre og slette alt innhold.

use std::sync::Arc;

use axum::extract::{Form, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use chrono::Utc;
use minijinja::context;
use serde::Deserialize;
use sqlx::Error;

use crate::db::post::{
    db_get_kladder, db_get_post, db_get_poster, db_get_publisert_post, db_oppdater_innhold, db_opprett_post,
    db_sett_publisert, db_slett_post, db_slug_finnes,
};
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

    // Alle kladder, ikke bare egne – autorisasjonen er flat, så enhver innlogget
    // forfatter skal se (og kunne redigere) alle utkast.
    let kladder: Vec<_> = match &author {
        Some(_) => db_get_kladder(&state.db)
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

    // Autorisasjonen er flat: enhver innlogget forfatter kan endre og slette.
    // Templaten bruker listen for å vise «Rediger»/«Slett» – utlogget er den tom.
    let actions: Vec<&str> = if author.is_some() {
        vec!["endre", "slett"]
    } else {
        vec![]
    };

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
            actions => actions,
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

/// Gjør en tittel om til en URL-slug: små bokstaver, norske tegn brettes til
/// ASCII (`æ→ae`, `ø→o`, `å→a`), alt annet enn bokstaver/tall blir bindestrek.
/// Repeterte bindestreker kollapses og kantene trimmes. En tittel som ikke gir
/// noe (f.eks. bare symboler) faller tilbake til `"innlegg"`.
///
/// Slugen genereres kun ved oppretting og låses etterpå – se PLAN.md.
fn slugifiser(tittel: &str) -> String {
    let mut tegn = String::with_capacity(tittel.len());
    for c in tittel.chars() {
        match c {
            'æ' | 'Æ' => tegn.push_str("ae"),
            'ø' | 'Ø' => tegn.push('o'),
            'å' | 'Å' => tegn.push('a'),
            c if c.is_ascii_alphanumeric() => tegn.extend(c.to_lowercase()),
            _ => tegn.push('-'),
        }
    }

    // Kollaps repeterte bindestreker i én passering. Starter «i skilletegn» så en
    // ledende bindestrek ikke dyttes inn – trimmen under er da kun for halen.
    let mut slug = String::with_capacity(tegn.len());
    let mut forrige_bindestrek = true;
    for c in tegn.chars() {
        if c == '-' {
            if !forrige_bindestrek {
                slug.push('-');
            }
            forrige_bindestrek = true;
        } else {
            slug.push(c);
            forrige_bindestrek = false;
        }
    }

    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "innlegg".to_string()
    } else {
        slug.to_string()
    }
}

/// Finner en ledig slug for en tittel: starter med `slugifiser`, og legger på
/// `-2`, `-3`, … hvis den alt finnes. Slugen låses ved oppretting – den endres
/// aldri når posten først er lagret.
async fn generer_ledig_slug(tittel: &str, db: &sqlx::PgPool) -> Result<String, sqlx::Error> {
    let base = slugifiser(tittel);
    let mut kandidat = base.clone();
    let mut n = 2;
    while db_slug_finnes(&kandidat, db).await? {
        kandidat = format!("{base}-{n}");
        n += 1;
    }
    Ok(kandidat)
}

/// Skjemaet fra editoren. `handling` er `name`-attributtet på submit-knappen:
/// `lagre` (eller fraværende), `publiser` eller `avpubliser`. `Option` fordi en
/// implicit submit (Enter i et felt) ikke sender noen knapp.
#[derive(Deserialize)]
pub struct PostSkjema {
    pub tittel: String,
    pub innhold: String,
    pub handling: Option<String>,
}

/// 303-redirect. 303 (ikke 302) tvinger nettleseren til å følge med GET, så en
/// refresh ikke re-poster skjemaet – samme grunn som i `auth.rs`.
fn redirect(location: &str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::LOCATION, HeaderValue::from_str(location).expect("gyldig sti"));
    (StatusCode::SEE_OTHER, headers).into_response()
}

const NY_POST_MAL: &str = "Skriv her.\n\n\
                           Forhåndsvisningen til høyre oppdateres mens du skriver.\n";

/// Ny post: tom editor. Krever innlogging – uten sesjon redirecter vi til
/// `/logg-inn` framfor å vise et skjema som likevel ikke kan lagres.
pub async fn ny_post(
    State(state): State<Arc<AppState>>,
    author: Option<AuthenticatedAuthor>,
) -> Result<Response, StatusCode> {
    if author.is_none() {
        return Ok(redirect("/logg-inn"));
    }

    let html = rendre(
        &state,
        "editor.html",
        context! {
            overskrift => "Ny post",
            handling => "/ny",
            avbryt => "/",
            slug => "",
            tittel => "",
            innhold => NY_POST_MAL,
            publisert => false,
            paalogget => true,
        },
    )?;
    Ok(html.into_response())
}

/// Rediger eksisterende post: samme editor, forhåndsfylt. Krever innlogging.
/// `publisert` styrer om editoren viser «Publiser» (kladd) eller «Avpubliser».
pub async fn rediger_post(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
    author: Option<AuthenticatedAuthor>,
) -> Result<Response, StatusCode> {
    if author.is_none() {
        return Ok(redirect("/logg-inn"));
    }

    let rad = db_get_post(&slug, &state.db).await.map_err(|e| match e {
        Error::RowNotFound => StatusCode::NOT_FOUND,
        e => db_feil(e),
    })?;

    // Avbryt peker tilbake dit posten faktisk lever: den publiserte siden, eller
    // forsiden for en kladd (en kladd har ingen offentlig `/post/{slug}`).
    let publisert = rad.published_at.is_some();
    let avbryt = if publisert {
        format!("/post/{}", rad.slug)
    } else {
        "/".to_string()
    };

    let html = rendre(
        &state,
        "editor.html",
        context! {
            overskrift => format!("Rediger «{}»", rad.title),
            handling => format!("/rediger/{}", rad.slug),
            avbryt => avbryt,
            slug => rad.slug,
            tittel => rad.title,
            innhold => rad.content,
            publisert => publisert,
            paalogget => true,
        },
    )?;
    Ok(html.into_response())
}

/// POST /ny – oppretter posten, og publiserer den i samme operasjon hvis
/// «Publiser»-knappen ble brukt. Slugen genereres her og låses for godt.
pub async fn opprett_post(
    State(state): State<Arc<AppState>>,
    author: Option<AuthenticatedAuthor>,
    Form(skjema): Form<PostSkjema>,
) -> Result<Response, StatusCode> {
    let Some(author) = author else {
        return Ok(redirect("/logg-inn"));
    };

    let publiser = skjema.handling.as_deref() == Some("publiser");
    let slug = generer_ledig_slug(&skjema.tittel, &state.db).await.map_err(db_feil)?;
    db_opprett_post(&slug, &skjema.tittel, &skjema.innhold, author.id, publiser, &state.db)
        .await
        .map_err(db_feil)?;

    Ok(if publiser {
        redirect(&format!("/post/{slug}"))
    } else {
        redirect(&format!("/rediger/{slug}?lagret=1"))
    })
}

/// POST /rediger/{slug} – lagrer tittel og innhold, og publiserer/avpubliserer
/// hvis den knappen ble brukt. «Lagre» rører ikke `published_at`.
pub async fn oppdater_post(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
    author: Option<AuthenticatedAuthor>,
    Form(skjema): Form<PostSkjema>,
) -> Result<Response, StatusCode> {
    let Some(author) = author else {
        return Ok(redirect("/logg-inn"));
    };

    // 404 hvis slugen mangler – `RETURNING slug` gir `RowNotFound`.
    db_oppdater_innhold(&slug, &skjema.tittel, &skjema.innhold, author.id, &state.db)
        .await
        .map_err(|e| match e {
            Error::RowNotFound => StatusCode::NOT_FOUND,
            e => db_feil(e),
        })?;

    let handling = skjema.handling.as_deref();
    match handling {
        Some("publiser") => {
            db_sett_publisert(&slug, Some(Utc::now()), author.id, &state.db)
                .await
                .map_err(db_feil)?;
        }
        Some("avpubliser") => {
            db_sett_publisert(&slug, None, author.id, &state.db)
                .await
                .map_err(db_feil)?;
        }
        _ => {} // «lagre» beholder publiseringsstatusen.
    }

    Ok(if handling == Some("publiser") {
        redirect(&format!("/post/{slug}"))
    } else {
        redirect(&format!("/rediger/{slug}?lagret=1"))
    })
}

/// GET /slett/{slug} – bekreftelsesside før sletting. Ren HTML, ingen JavaScript:
/// sletting er irreversibelt, så et ekstra klikk er billig forsikring.
pub async fn slett_bekreft(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
    author: Option<AuthenticatedAuthor>,
) -> Result<Response, StatusCode> {
    if author.is_none() {
        return Ok(redirect("/logg-inn"));
    }

    let rad = db_get_post(&slug, &state.db).await.map_err(|e| match e {
        Error::RowNotFound => StatusCode::NOT_FOUND,
        e => db_feil(e),
    })?;

    let html = rendre(
        &state,
        "slett.html",
        context! {
            slug => rad.slug,
            tittel => rad.title,
            paalogget => true,
        },
    )?;
    Ok(html.into_response())
}

/// POST /slett/{slug} – sletter posten for godt.
pub async fn slett_post(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
    author: Option<AuthenticatedAuthor>,
) -> Result<Response, StatusCode> {
    let Some(_) = author else {
        return Ok(redirect("/logg-inn"));
    };

    db_slett_post(&slug, &state.db).await.map_err(|e| match e {
        Error::RowNotFound => StatusCode::NOT_FOUND,
        e => db_feil(e),
    })?;

    Ok(redirect("/"))
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

    #[test]
    fn slugifiser_brekker_norske_tegn() {
        assert_eq!(slugifiser("Cusco og høydesyke"), "cusco-og-hoydesyke");
        assert_eq!(slugifiser("Ærfugl Øy Ås"), "aerfugl-oy-as");
    }

    #[test]
    fn slugifiser_kollapser_og_trimmer_bindestreker() {
        assert_eq!(slugifiser("  To   ord!  "), "to-ord");
        assert_eq!(slugifiser("Hei,  verden!!!"), "hei-verden");
    }

    #[test]
    fn slugifiser_takler_store_bokstaver_og_tall() {
        assert_eq!(slugifiser("Tur 2026 til Lima"), "tur-2026-til-lima");
    }

    #[test]
    fn slugifiser_faller_tilbake_pa_innlegg() {
        assert_eq!(slugifiser(""), "innlegg");
        assert_eq!(slugifiser("..."), "innlegg");
        assert_eq!(slugifiser("  ––  "), "innlegg");
    }
}
