//! Auth: innlogging med engangskode og sesjon-cookie.
//!
//! Flyten er hentet fra fagord-rust-api (`auth.rs`), men tilpasset monolitten:
//! der svarte API-et med JSON og lot en React Router-server sette cookie-en. Her
//! svarer vi med HTML og setter `Set-Cookie` selv, så `verify` ender i en
//! redirect i stedet for en `{session_token, ...}`-payload.

use std::sync::Arc;

use axum::extract::{Form, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use chrono::{DateTime, Utc};
use minijinja::context;
use serde::Deserialize;

use crate::db::magic_token::{
    db_find_active_token, db_find_author_by_email, db_mark_token_used, db_register_attempt, db_replace_magic_token,
};
use crate::db::session::{db_create_session, db_delete_session};
use crate::email::send_magic_kode;
use crate::extract::{SESSION_COOKIE, SessionCookie};
use crate::tokens::{generate_code, generate_session_token, hash_token};
use crate::{AppState, rendre};

/// Maks antall verifiseringsforsøk per kode. Sammen med kort levetid er sperren det
/// som gjør den korte koden trygg mot gjetting. Reserveres atomisk i `db_register_attempt`.
const MAX_ATTEMPTS: i32 = 5;

#[derive(Deserialize)]
pub struct MagicRequest {
    pub epost: String,
}

#[derive(Deserialize)]
pub struct VerifyRequest {
    pub epost: String,
    pub kode: String,
}

/// GET /logg-inn – skjema for e-post.
pub async fn logg_inn_side(State(state): State<Arc<AppState>>) -> Result<Html<String>, StatusCode> {
    rendre(
        &state,
        "logg-inn.html",
        context! {
            kode_sendt => false,
            epost => "",
            feil => "",
        },
    )
}

/// POST /logg-inn – be om en engangskode.
///
/// Svarer alltid med «sjekk e-posten din»-siden, uansett om e-posten finnes, så
/// endepunktet ikke kan brukes til å kartlegge registrerte e-poster.
pub async fn request_magic_code(
    State(state): State<Arc<AppState>>,
    Form(payload): Form<MagicRequest>,
) -> Result<Html<String>, StatusCode> {
    let epost = payload.epost;

    match db_find_author_by_email(&epost, &state.db).await {
        Ok(Some(author)) => {
            let kode = generate_code();
            match db_replace_magic_token(author.id, &hash_token(&kode), &state.db).await {
                Ok(()) => send_magic_kode(&state.env, &epost, &kode).await,
                Err(e) => tracing::error!("Feil ved lagring av magic-kode: {e:#?}"),
            }
        }
        // Ukjent e-post: stopp stille, men svar som om alt gikk bra.
        Ok(None) => tracing::info!("Magic-kode bedt om for ukjent e-post: {epost:?}"),
        Err(e) => tracing::error!("Feil ved oppslag av forfatter: {e:#?}"),
    }

    rendre(
        &state,
        "logg-inn.html",
        context! {
            kode_sendt => true,
            epost => epost,
            feil => "",
        },
    )
}

/// POST /logg-inn/verifiser – verifiser en engangskode.
///
/// Alle feil (ukjent e-post, ingen/feil/utløpt kode, for mange forsøk) viser
/// kode-skjemaet igjen med samme feilmelding, så endepunktet ikke røper hvor i
/// flyten det glapp. Ved suksess forbrukes koden, en sesjon opprettes, og vi
/// setter cookie-en og redirecter til forsiden.
pub async fn verify_magic_code(
    State(state): State<Arc<AppState>>,
    Form(payload): Form<VerifyRequest>,
) -> Result<Response, StatusCode> {
    match verifiser_kode(&state, &payload.epost, &payload.kode).await {
        Ok((token, expires_at)) => Ok(redirect_med_cookie("/", &session_cookie(&token, expires_at))),
        Err(()) => {
            let html = rendre(
                &state,
                "logg-inn.html",
                context! {
                    kode_sendt => true,
                    epost => payload.epost,
                    feil => "Feil kode eller koden er utløpt. Prøv igjen.",
                },
            )?;
            Ok(html.into_response())
        }
    }
}

/// POST /logg-ut – sletter sesjonsraden og fjerner cookie-en. Redirecter til
/// forsiden.
pub async fn logg_ut(
    State(state): State<Arc<AppState>>,
    SessionCookie(token): SessionCookie,
) -> Result<Response, StatusCode> {
    match db_delete_session(&hash_token(&token), &state.db).await {
        Ok(n) => tracing::info!("Utlogging: {n} sesjon(er) slettet"),
        Err(e) => tracing::error!("Feil ved sletting av sesjon: {e:#?}"),
    }

    Ok(redirect_med_cookie("/", &clear_cookie()))
}

/// Kjernen i verifiseringen: forsøkssperre → sjekk kode → forbruk → sesjon.
/// `Ok` gir det rå sesjonstokenet (som kun vises denne ene gangen) og utløpstiden.
async fn verifiser_kode(state: &AppState, epost: &str, kode: &str) -> Result<(String, DateTime<Utc>), ()> {
    let db = &state.db;

    let author = db_find_author_by_email(epost, db)
        .await
        .map_err(|e| tracing::error!("Feil ved oppslag av forfatter: {e:#?}"))?
        .ok_or(())?;

    let token = db_find_active_token(author.id, db)
        .await
        .map_err(|e| tracing::error!("Feil ved oppslag av magic-kode: {e:#?}"))?
        .ok_or(())?;

    // Reserver et forsøk *før* vi sjekker koden. `None` betyr at grensen alt er nådd.
    let Some(_forsøk) = db_register_attempt(token.id, MAX_ATTEMPTS, db)
        .await
        .map_err(|e| tracing::error!("Feil ved registrering av forsøk: {e:#?}"))?
    else {
        tracing::warn!("Magic-kode avvist – forsøksgrensen nådd (forfatter {})", author.id);
        return Err(());
    };

    if hash_token(kode) != token.token_hash {
        tracing::info!("Feil magic-kode");
        return Err(());
    }

    // Riktig kode: forbruk den (engangsbruk) før vi utsteder sesjonen.
    db_mark_token_used(token.id, db)
        .await
        .map_err(|e| tracing::error!("Feil ved forbruk av magic-kode: {e:#?}"))?;

    let session_token = generate_session_token();
    let session = db_create_session(author.id, &hash_token(&session_token), db)
        .await
        .map_err(|e| tracing::error!("Feil ved oppretting av sesjon: {e:#?}"))?;

    tracing::info!(
        "Forfatter {} verifisert via magic-kode – sesjon {} utstedt",
        author.id,
        session.id
    );

    Ok((session_token, session.expires_at))
}

/// Setter `Secure` kun når `COOKIE_SECURE=1`. I lokal utvikling går trafikken over
/// `http://127.0.0.1`, og en `Secure`-cookie nektes av nettleseren – da feiler
/// innloggingen stille. I prod settes `COOKIE_SECURE=1` slik at cookie-en kun sendes
/// over HTTPS.
fn cookie_secure() -> bool {
    std::env::var("COOKIE_SECURE").is_ok_and(|v| v == "1")
}

/// Sesjon-cookie-en: `HttpOnly` (leses ikke av JS), `SameSite=Lax` (CSRF-vern for
/// skjema-POST), `Path=/`, `Max-Age` = sesjonens resterende levetid.
fn session_cookie(token: &str, expires_at: DateTime<Utc>) -> String {
    let max_age = (expires_at - chrono::Utc::now()).num_seconds().max(0);
    let secure = if cookie_secure() { "; Secure" } else { "" };
    format!("{SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age}{secure}")
}

/// Fjerner cookie-en ved å sette `Max-Age=0`.
fn clear_cookie() -> String {
    let secure = if cookie_secure() { "; Secure" } else { "" };
    format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0{secure}")
}

/// 303-redirect med en `Set-Cookie`-header. 303 (ikke 302) tvinger nettleseren til
/// å følge redirecten med GET, så en refresh ikke re-poster skjemaet.
fn redirect_med_cookie(location: &str, cookie: &str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::LOCATION, HeaderValue::from_str(location).expect("gyldig sti"));
    headers.insert(
        header::SET_COOKIE,
        HeaderValue::from_str(cookie).expect("gyldig cookie"),
    );
    (StatusCode::SEE_OTHER, headers).into_response()
}
