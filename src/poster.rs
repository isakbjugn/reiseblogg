//! Poster: data og handlere for lesesidene og editoren.
//!
//! Postene er hardkodet i `alle_poster()` inntil databasen kommer i steg 2. Formen
//! på `Post` er valgt for å matche `post`-tabellen i planen, så byttet blir å
//! erstatte én funksjon – ikke å endre handlerne.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Html;
use minijinja::context;

use crate::markdown;
use crate::{AppState, rendre};

/// En bloggpost. Speiler `post`-tabellen fra PLAN.md, uten id-ene ennå.
pub struct Post {
    pub slug: &'static str,
    pub tittel: &'static str,
    /// ISO-format (`YYYY-MM-DD`). Blir `published_at` i databasen.
    pub dato: &'static str,
    pub forfatter: &'static str,
    /// Markdown, ikke HTML. Rendres av `markdown::til_html` ved visning.
    pub innhold: &'static str,
}

/// Stand-in for databasen. Nyeste først – samme rekkefølge som
/// `ORDER BY published_at DESC` vil gi.
fn alle_poster() -> Vec<Post> {
    vec![
        Post {
            slug: "cusco-og-hoydesyke",
            tittel: "Cusco og høydesyke",
            dato: "2026-08-02",
            forfatter: "Isak",
            innhold: "Vi kom til Cusco i går kveld, 3400 meter over havet, og \
                      merket det med en gang. Ikke dramatisk -- bare en dump \
                      hodepine og en puls som lå høyere enn den burde.\n\n\
                      ## Cocatea og tidlig kveld\n\n\
                      Verten på hostellet kokte cocatea uten å bli spurt. Det \
                      hjelper visst, og vi la oss klokka ni som to pensjonister.\n\n\
                      - Drikk mer vann enn du tror du trenger\n\
                      - Ikke gå fort oppover trapper\n\
                      - Ta det første døgnet rolig\n\n\
                      I morgen skal vi bare gå rundt i byen. **Machu Picchu** får \
                      vente til kroppen har vent seg til luften.",
        },
        Post {
            slug: "buss-gjennom-atacama",
            tittel: "Buss gjennom Atacama",
            dato: "2026-07-28",
            forfatter: "Ingrid",
            innhold: "Fjorten timer i buss gjennom verdens tørreste ørken. Det \
                      høres verre ut enn det var.\n\n\
                      Landskapet skifter fra rustrødt til nesten hvitt, og så \
                      tilbake igjen, og du sitter og ser på det uten å kjede deg. \
                      Ingen trær. Ingen skilt. En og annen bil i motsatt retning.\n\n\
                      > Det nærmeste jeg har vært en annen planet.\n\n\
                      Vi stoppet i San Pedro sent på kvelden. Stjernehimmelen der \
                      er den beste jeg har sett -- ingen lys på mange mil.",
        },
        Post {
            slug: "forste-dag-i-bogota",
            tittel: "Første dag i Bogotá",
            dato: "2026-07-21",
            forfatter: "Isak",
            innhold: "Vi landet i går, og har brukt dagen på å ikke gjøre særlig \
                      mye. Det var planen.\n\n\
                      Bogotá ligger på 2600 meter, så vi rakk å bli litt \
                      andpustne av å gå oppover mot La Candelaria. Byen er \
                      større enn jeg hadde forestilt meg, og kaldere.\n\n\
                      ## Det vi spiste\n\n\
                      *Ajiaco* -- en suppe med kylling, mais og poteter, servert \
                      med avokado og kapers ved siden av. Anbefales når du er \
                      kald og trøtt etter en nattflyvning.\n\n\
                      Fire måneder gjenstår.",
        },
    ]
}

/// Slår opp én post på slug.
fn finn_post(slug: &str) -> Option<Post> {
    alle_poster().into_iter().find(|p| p.slug == slug)
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
pub async fn forside(State(state): State<Arc<AppState>>) -> Result<Html<String>, StatusCode> {
    let poster: Vec<_> = alle_poster()
        .iter()
        .map(|p| {
            context! {
                slug => p.slug,
                tittel => p.tittel,
                dato => p.dato,
                dato_lesbar => norsk_dato(p.dato),
                forfatter => p.forfatter,
                sammendrag => markdown::sammendrag(p.innhold, 180),
            }
        })
        .collect();

    rendre(&state, "forside.html", context! { poster })
}

/// En enkelt post. Markdown rendres her, ikke i templaten.
pub async fn vis_post(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> Result<Html<String>, StatusCode> {
    let post = finn_post(&slug).ok_or(StatusCode::NOT_FOUND)?;

    rendre(
        &state,
        "post.html",
        context! {
            slug => post.slug,
            tittel => post.tittel,
            dato => post.dato,
            dato_lesbar => norsk_dato(post.dato),
            forfatter => post.forfatter,
            innhold => markdown::til_html(post.innhold),
            // Til <meta description> og Open Graph.
            sammendrag => markdown::sammendrag(post.innhold, 180),
        },
    )
}

/// Arkiv: alle poster gruppert på år og måned.
pub async fn arkiv(State(state): State<Arc<AppState>>) -> Result<Html<String>, StatusCode> {
    // Postene er allerede sortert nyeste først, så vi kan gruppere sekvensielt og
    // slipper å sortere gruppene etterpå.
    let mut grupper: Vec<minijinja::Value> = Vec::new();
    let mut nåværende: Option<(i32, u32, Vec<minijinja::Value>)> = None;

    for post in alle_poster() {
        let (år, måned, _) = del_dato(post.dato).unwrap_or((0, 0, 0));

        let oppføring = context! {
            slug => post.slug,
            tittel => post.tittel,
            dato => post.dato,
            dato_lesbar => norsk_dato(post.dato),
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

    rendre(&state, "arkiv.html", context! { grupper })
}

/// Om oss – statisk side.
pub async fn om_oss(State(state): State<Arc<AppState>>) -> Result<Html<String>, StatusCode> {
    rendre(&state, "om-oss.html", context! {})
}

const NY_POST_MAL: &str = "Skriv her.\n\n\
                           Forhåndsvisningen til høyre oppdateres mens du skriver.\n";

/// Ny post: tom editor.
pub async fn ny_post(State(state): State<Arc<AppState>>) -> Result<Html<String>, StatusCode> {
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
        },
    )
}

/// Rediger eksisterende post: samme editor, forhåndsfylt.
///
/// Lagring kommer i steg 4 – handleren viser bare skjemaet nå.
pub async fn rediger_post(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> Result<Html<String>, StatusCode> {
    let post = finn_post(&slug).ok_or(StatusCode::NOT_FOUND)?;

    rendre(
        &state,
        "editor.html",
        context! {
            overskrift => format!("Rediger «{}»", post.tittel),
            handling => format!("/rediger/{}", post.slug),
            avbryt => format!("/post/{}", post.slug),
            slug => post.slug,
            tittel => post.tittel,
            innhold => post.innhold,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poster_er_sortert_nyeste_forst() {
        let poster = alle_poster();
        let datoer: Vec<_> = poster.iter().map(|p| p.dato).collect();
        let mut sortert = datoer.clone();
        sortert.sort_unstable_by(|a, b| b.cmp(a));
        assert_eq!(datoer, sortert, "forsiden viser dem i denne rekkefølgen");
    }

    #[test]
    fn slugs_er_unike() {
        let poster = alle_poster();
        let mut slugs: Vec<_> = poster.iter().map(|p| p.slug).collect();
        slugs.sort_unstable();
        let antall = slugs.len();
        slugs.dedup();
        assert_eq!(slugs.len(), antall, "slug må være unik – den er nøkkelen i URL-en");
    }

    #[test]
    fn finn_post_treffer_og_bommer() {
        assert!(finn_post("cusco-og-hoydesyke").is_some());
        assert!(finn_post("finnes-ikke").is_none());
    }

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
