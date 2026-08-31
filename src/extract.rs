//! Egendefinerte Axum-extractors for auth.
//!
//! Forskjellen fra fagord-rust-api: der var det et Bearer-token i en
//! `Authorization`-header, båret av en React Router-server i midten. Her er
//! appen en monolitt, så tokenet ligger i en `HttpOnly`-cookie satt av oss selv.

use axum::extract::{FromRequestParts, OptionalFromRequestParts};
use axum::http::StatusCode;
use axum::http::request::Parts;
use sqlx::PgPool;

use crate::db::session::db_find_session_author;
use crate::tokens::hash_token;

/// Navnet på sesjon-cookie-en. Leses av `SessionCookie`, settes av `auth.rs`.
pub const SESSION_COOKIE: &str = "sesjon";

/// Gir extractors tilgang til databasen uten å kjenne hele app-staten. Egen trait
/// (ikke axum sin `FromRef`) fordi orphan-regelen hindrer oss i å impl-e en fremmed
/// trait for `PgPool` når staten er `Arc<AppState>`.
pub trait HasDb {
    fn db(&self) -> &PgPool;
}

/// Det rå sesjonstokenet fra cookie-en. Henter kun strengen – slår ikke opp
/// sesjonen. Selve oppslaget kommer som egen extractor (`AuthenticatedAuthor`).
#[derive(Debug)]
pub struct SessionCookie(pub String);

impl<S> FromRequestParts<S> for SessionCookie
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(axum::http::header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .and_then(|header| hent_cookie(header, SESSION_COOKIE))
            .ok_or(StatusCode::UNAUTHORIZED)?;

        Ok(SessionCookie(token))
    }
}

/// Finner verdien av én cookie i en `Cookie`-header.
///
/// Cookie-formatet er `navn=verdi; navn2=verdi2`. Sesjonstokenet vårt er en
/// hex-streng uten spesialtegn, så en enkel split er nok – vi tar ikke inn en
/// cookie-crate (`axum-extra` + `cookie`) for noe så enkelt. Se CLAUDE.md om
/// bevisst bruk av abstraksjoner.
fn hent_cookie(header: &str, navn: &str) -> Option<String> {
    header.split(';').find_map(|del| {
        let (nøkkel, verdi) = del.trim().split_once('=')?;
        (nøkkel == navn).then(|| verdi.to_string())
    })
}

/// En autentisert forfatter, slått opp fra sesjon-cookie-en. Handlere som tar
/// denne som argument er beskyttet: en manglende/ugyldig/utløpt sesjon gir `401`
/// før handleren kjører. Ingen roller – alle innloggede har lik tilgang – så kun
/// `id` trengs.
#[derive(Debug)]
pub struct AuthenticatedAuthor {
    pub id: i32,
}

impl<S> FromRequestParts<S> for AuthenticatedAuthor
where
    S: HasDb + Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let SessionCookie(token) = SessionCookie::from_request_parts(parts, state).await?;

        let author = db_find_session_author(&hash_token(&token), state.db())
            .await
            .map_err(|e| {
                tracing::error!("Feil ved oppslag av sesjon: {:#?}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?
            .ok_or(StatusCode::UNAUTHORIZED)?;

        Ok(AuthenticatedAuthor { id: author.id })
    }
}

/// `Option<AuthenticatedAuthor>`: `None` når sesjonen mangler eller er utløpt,
/// `Some` når brukeren er logget inn. Lesesidene bruker den for å vise kladder
/// og «Skriv»-lenken kun for innloggede, uten å kreve innlogging.
impl<S> OptionalFromRequestParts<S> for AuthenticatedAuthor
where
    S: HasDb + Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Option<Self>, Self::Rejection> {
        match <Self as FromRequestParts<S>>::from_request_parts(parts, state).await {
            Ok(author) => Ok(Some(author)),
            Err(StatusCode::UNAUTHORIZED) => Ok(None),
            Err(other) => Err(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    #[test]
    fn henter_cookie_fra_header() {
        let header = "tema=mørk; sesjon=abc123; språk=nb";
        assert_eq!(hent_cookie(header, "sesjon"), Some("abc123".to_string()));
    }

    #[test]
    fn henter_cookie_når_den_er_først_eller_sist() {
        assert_eq!(hent_cookie("sesjon=abc123", "sesjon"), Some("abc123".to_string()));
        assert_eq!(hent_cookie("a=1; sesjon=abc123", "sesjon"), Some("abc123".to_string()));
    }

    #[test]
    fn manglende_cookie_gir_none() {
        assert_eq!(hent_cookie("tema=mørk", "sesjon"), None);
        assert_eq!(hent_cookie("", "sesjon"), None);
    }

    fn parts_med_cookie(header: Option<&str>) -> Parts {
        let mut builder = Request::builder();
        if let Some(value) = header {
            builder = builder.header(axum::http::header::COOKIE, value);
        }
        builder.body(()).unwrap().into_parts().0
    }

    async fn trekk_ut(header: Option<&str>) -> Result<SessionCookie, StatusCode> {
        let mut parts = parts_med_cookie(header);
        SessionCookie::from_request_parts(&mut parts, &()).await
    }

    #[tokio::test]
    async fn trekker_token_fra_cookie() {
        let SessionCookie(token) = trekk_ut(Some("sesjon=abc123")).await.unwrap();
        assert_eq!(token, "abc123");
    }

    #[tokio::test]
    async fn manglende_cookie_gir_401() {
        assert_eq!(trekk_ut(None).await.unwrap_err(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn feil_cookie_navn_gir_401() {
        assert_eq!(
            trekk_ut(Some("annen=abc123")).await.unwrap_err(),
            StatusCode::UNAUTHORIZED
        );
    }
}
