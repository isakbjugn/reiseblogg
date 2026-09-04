//! Reiseblogg – én Rust-app som rendrer HTML og serverer editoren.
//!
//! Postene kommer fra databasen (`db::post`). Innloggede forfattere skriver,
//! publiserer og sletter via editoren og skjema-POST-ene i `poster`.
//!
//! Tre valg som ligger til grunn:
//!
//! 1. Lesesidene rendres som ren HTML – null JavaScript, indekserbart.
//! 2. Editoren er en Preact-island: én `<div>` som Preact monterer seg i.
//! 3. Ingen byggekjede. Nettleseren laster ES-moduler direkte, og `cargo run`
//!    er hele oppsettet.

mod auth;
mod db;
mod email;
mod extract;
mod markdown;
mod media;
mod poster;
mod rate_limit;
mod tokens;
mod types;

use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::from_fn_with_state;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use minijinja::{Environment, context, path_loader};
use sqlx::postgres::PgPoolOptions;
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::extract::AuthenticatedAuthor;

/// Delt tilstand: databasepoolen for post-lesing og minijinja-miljøet.
pub struct AppState {
    db: sqlx::PgPool,
    env: Environment<'static>,
    s3: aws_sdk_s3::Client,
}

/// Lar extractors hente databasen ut av staten uten å kjenne hele AppState.
impl extract::HasDb for Arc<AppState> {
    fn db(&self) -> &sqlx::PgPool {
        &self.db
    }
}

/// Bygger minijinja-miljøet.
///
/// `path_loader` leser templates fra `templates/` ved kjøring i stedet for å
/// kompilere dem inn i binæret (som askama/rinja gjør). Det koster typesikkerhet
/// i templatene, men holder HTML-en i .html-filer med syntaksfarging og
/// formatering – og gjør dem redigerbare uten å røre Rust-koden.
///
/// Merk at minijinja cacher hver template etter første innlasting, så en redigert
/// fil får ingen effekt i en kjørende prosess. Det er `cargo watch -w templates/`
/// som løser det, ved å restarte prosessen – ikke noe i koden her. Se README.
fn build_env() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_loader(path_loader("templates"));
    env
}

/// Rendrer en template med gitt kontekst.
///
/// `pub(crate)` fordi handlerne ligger i `poster`-modulen.
pub(crate) fn rendre(state: &AppState, navn: &str, ctx: minijinja::Value) -> Result<Html<String>, StatusCode> {
    let template = state
        .env
        .get_template(navn)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    template
        .render(ctx)
        .map(Html)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// 404-side for ukjente URL-er.
///
/// Egen handler framfor axums standardsvar, så en feilstavet lenke gir en side med
/// navigasjon tilbake – ikke en tom `Not Found` i klartekst.
async fn ikke_funnet(State(state): State<Arc<AppState>>, author: Option<AuthenticatedAuthor>) -> Response {
    match rendre(&state, "404.html", context! { paalogget => author.is_some() }) {
        Ok(html) => (StatusCode::NOT_FOUND, html).into_response(),
        // Feiler selve feilsiden, vil vi ikke skjule det bak en 500.
        Err(_) => (StatusCode::NOT_FOUND, "Siden finnes ikke").into_response(),
    }
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let db = PgPoolOptions::new()
        .max_connections(5)
        .connect(&std::env::var("DATABASE_URL").expect("DATABASE_URL må være satt"))
        .await
        .expect("Klarte ikke å koble til databasen");

    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("Klarte ikke å gjøre database-migrering");

    let state = Arc::new(AppState { db, env: build_env(), s3: media::s3_client() });

    // Rate-limiter for offentlig auth-trafikk. I lokal utvikling (ingen edge-header)
    // er den en no-op; se rate_limit.rs for begrunnelsen.
    let limiter = rate_limit::build_limiter();
    rate_limit::spawn_cleanup(limiter.clone());

    // Auth-rutene i egen sub-router, så strupingen (route_layer) kun treffer disse,
    // ikke resten av siden.
    let auth_routes = Router::new()
        .route("/logg-inn", get(auth::logg_inn_side).post(auth::request_magic_code))
        .route("/logg-inn/verifiser", post(auth::verify_magic_code))
        .route("/logg-ut", post(auth::logg_ut))
        .route_layer(from_fn_with_state(limiter, rate_limit::rate_limit_external))
        .with_state(state.clone());

    // Egenhostede avhengigheter er versjonspinnet og uforanderlige, så de kan caches
    // hardt. `ServeDir` (tower-http `fs`-featuren) serverer dem – ingenting går ut
    // på nettet fra nettleseren.
    let uforanderlig = SetResponseHeaderLayer::overriding(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    let vendor_med_cache = Router::new()
        .fallback_service(ServeDir::new("static/vendor"))
        .layer(uforanderlig);

    // `stil.css` og `editor.js` er mutable – de endres mellom deployer, så de kan ikke
    // caches hardt som vendor-filene. `no-cache` lar nettleseren lagre dem, men
    // revalidere mot `ETag`/`Last-Modified` (som `ServeDir` setter) før bruk. Uten
    // dette hang en gammel `stil.css` igjen i cachen etter auth-PR-en, så
    // logg-inn-skjemaet ble vist ustylet i produksjon.
    let mutbar = SetResponseHeaderLayer::overriding(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    let static_med_cache = Router::new().fallback_service(ServeDir::new("static")).layer(mutbar);

    // Komprimering på ALT: uten dette ville markdown-it gått som 147 kB rå i stedet
    // for 44 kB brotli. `ServeDir` gjør ikke dette av seg selv.
    let app = Router::new()
        .route("/", get(poster::forside))
        .route("/post/{slug}", get(poster::vis_post))
        .route("/arkiv", get(poster::arkiv))
        .route("/om-oss", get(poster::om_oss))
        .route("/ny", get(poster::ny_post).post(poster::opprett_post))
        .route("/rediger/{slug}", get(poster::rediger_post).post(poster::oppdater_post))
        .route("/slett/{slug}", get(poster::slett_bekreft).post(poster::slett_post))
        .route("/media", post(media::create_media))
        .merge(auth_routes)
        .nest_service("/static/vendor", vendor_med_cache)
        .nest_service("/static", static_med_cache)
        .fallback(ikke_funnet)
        .layer(CompressionLayer::new().br(true).gzip(true))
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let adresse = format!("0.0.0.0:{port}");

    let listener = tokio::net::TcpListener::bind(&adresse)
        .await
        .unwrap_or_else(|_| panic!("port {port} må være ledig"));

    tracing::info!("Kjører på http://{adresse}");
    axum::serve(listener, app).await.expect("serveren stoppet");
}
