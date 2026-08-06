//! Spike: beviser at minijinja + Preact-island + vendrede avhengigheter henger sammen.
//!
//! Ingen database, ingen faktiske bloggposter. Poenget er å vise tre ting:
//!
//! 1. Lesesidene rendres av Rust som ren HTML – null JavaScript, indekserbart.
//! 2. Editoren er en Preact-island: én `<div>` som Preact monterer seg i.
//! 3. Ingen byggekjede. `cargo run` er hele oppsettet; templates leses fra disk
//!    ved kjøring, så HTML-endringer krever bare refresh i nettleseren.

use std::sync::Arc;

use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Html;
use axum::routing::get;
use minijinja::{Environment, context, path_loader};
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

/// Delt tilstand. I den ekte appen kommer `PgPool` inn her ved siden av `env`.
struct AppState {
    env: Environment<'static>,
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
fn rendre(
    state: &AppState,
    navn: &str,
    ctx: minijinja::Value,
) -> Result<Html<String>, StatusCode> {
    let template = state
        .env
        .get_template(navn)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    template
        .render(ctx)
        .map(Html)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// Stand-in for det databasen skal levere senere.
fn fake_poster() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("cusco-og-hoydesyke", "Cusco og høydesyke", "2026-08-02"),
        ("buss-gjennom-atacama", "Buss gjennom Atacama", "2026-07-28"),
        ("forste-dag-i-bogota", "Første dag i Bogotá", "2026-07-21"),
    ]
}

/// Forsiden: ren server-rendret HTML. Ingen JS lastes på denne siden.
async fn forside(State(state): State<Arc<AppState>>) -> Result<Html<String>, StatusCode> {
    let poster: Vec<_> = fake_poster()
        .into_iter()
        .map(|(slug, tittel, dato)| context! { slug, tittel, dato })
        .collect();

    rendre(&state, "forside.html", context! { poster })
}

/// En enkelt post – også ren HTML. Beviser at interpolering per post fungerer,
/// som er nettopp grunnen til at vi trenger en template-motor (jp-tools, som er
/// en full SPA, trenger ingen).
async fn vis_post(
    State(state): State<Arc<AppState>>,
    Path(ønsket_slug): Path<String>,
) -> Result<Html<String>, StatusCode> {
    let (slug, tittel, dato) = fake_poster()
        .into_iter()
        .find(|(s, _, _)| *s == ønsket_slug)
        .ok_or(StatusCode::NOT_FOUND)?;

    rendre(
        &state,
        "post.html",
        context! {
            slug,
            tittel,
            dato,
            // Rendret Markdown kommer hit senere (pulldown-cmark). Nå: fast tekst.
            innhold => "<p>Her kommer brødteksten når databasen er på plass.</p>",
        },
    )
}

/// Editoren: den ene siden som laster JavaScript. Rust rendrer skallet og legger
/// startverdiene i `data-`-attributter, så Preact ikke trenger et ekstra API-kall
/// for å vite hva den redigerer.
async fn editor(State(state): State<Arc<AppState>>) -> Result<Html<String>, StatusCode> {
    rendre(
        &state,
        "editor.html",
        context! {
            slug => "ny",
            tittel => "",
            innhold => "# Skriv her\n\nBrødtekst i **Markdown**. Forhåndsvisningen \
                        til høyre oppdateres mens du skriver – uten et kall til serveren.\n\n\
                        - Første punkt\n- Andre punkt\n",
        },
    )
}

#[tokio::main]
async fn main() {
    let state = Arc::new(AppState { env: build_env() });

    // Vendrede avhengigheter er versjonspinnet og uforanderlige, så de kan caches
    // hardt. `ServeDir` (tower-http `fs`-featuren) serverer dem – ingen CDN, så
    // ingenting går ut på nettet fra nettleseren.
    let vendor_med_cache = Router::new()
        .fallback_service(ServeDir::new("static/vendor"))
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        ));

    // Komprimering på ALT: uten dette serverte vi markdown-it som 147 kB rå der
    // esm.sh sender 43 kB brotli. Det er den faktiske kostnaden ved å hoste selv,
    // og den forsvinner med ett lag. `ServeDir` gjør ikke dette av seg selv.
    let app = Router::new()
        .route("/", get(forside))
        .route("/post/{slug}", get(vis_post))
        .route("/ny", get(editor))
        .nest_service("/static/vendor", vendor_med_cache)
        .nest_service("/static", ServeDir::new("static"))
        .layer(CompressionLayer::new().br(true).gzip(true))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .expect("port 8080 må være ledig");

    println!("Kjører på http://127.0.0.1:8080");
    axum::serve(listener, app).await.expect("serveren stoppet");
}
