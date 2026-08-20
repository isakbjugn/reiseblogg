//! E-post: sender engangskoden for innlogging.
//!
//! Mønsteret er hentet fra rust-auth (`utils/emails.rs`), men forenklet:
//! SMTP-oppsettet leses fra miljøvariabler i stedet for en egen `config`-crate,
//! og vi gjenbruker appens minijinja-miljø i stedet for å bygge et nytt.
//!
//! Uten `SMTP_HOST` satt logges koden i stedet for å sendes – samme mønster som
//! fagord-rust-api (der e-post er et senere byggesteg). Da fungerer hele
//! innloggingsflyten lokalt uten SMTP-kredentialer, og e-post blir en ren
//! konfigurasjonsflipp i prod.

use lettre::message::{Mailbox, MultiPart, SinglePart, header::ContentType};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use minijinja::{Environment, context};

/// Sender engangskoden til e-postadressen, eller logger den hvis SMTP ikke er
/// konfigurert (eller sending feiler). E-post er en sideeffekt – den kan ikke
/// feile selve innloggingsflyten, bare gjøre at koden ikke når fram.
pub async fn send_magic_kode(env: &Environment<'static>, epost: &str, kode: &str) {
    let (html, tekst) = match rend_epost(env, kode) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("Kunne ikke rendere e-posttemplate: {e:#?}");
            logg_kode(epost, kode);
            return;
        }
    };

    // I utvikling er SMTP_HOST usatt: logg koden framfor å sende.
    let Some(host) = std::env::var("SMTP_HOST").ok() else {
        logg_kode(epost, kode);
        return;
    };

    match send_smtp(&host, epost, &html, &tekst).await {
        Ok(()) => tracing::info!("Engangskode sendt til {epost}"),
        Err(e) => {
            tracing::error!("Kunne ikke sende e-post til {epost}: {e}");
            // Fallback så brukeren ikke står låst om SMTP svikter i prod.
            logg_kode(epost, kode);
        }
    }
}

/// Renderer HTML- og tekstversjonen av e-posten. HTML-en kommer fra
/// `templates/epost/magic-kode.html`; teksten er en enkel fallback for
/// e-postklienter uten HTML.
fn rend_epost(env: &Environment<'static>, kode: &str) -> Result<(String, String), minijinja::Error> {
    let html = env
        .get_template("epost/magic-kode.html")?
        .render(context! { kode => kode })?;

    let tekst = format!("Din innloggingskode til reisebloggen er: {kode}\n\nKoden utløper om 15 minutter.");

    Ok((html, tekst))
}

/// Sender en multipart-e-post (tekst + HTML) over SMTP. Oppsettet kommer fra
/// miljøvariabler: `SMTP_HOST`, `SMTP_USER`, `SMTP_PASSWORD`, `SMTP_FROM`,
/// `SMTP_FROM_NAME`. `SMTP_FROM` faller tilbake til `SMTP_USER` – typisk krever
/// leverandøren at avsenderen er den autentiserte brukeren.
async fn send_smtp(host: &str, epost: &str, html: &str, tekst: &str) -> Result<(), String> {
    let fra = std::env::var("SMTP_FROM")
        .or_else(|_| std::env::var("SMTP_USER"))
        .unwrap_or_default();
    let fra_navn = std::env::var("SMTP_FROM_NAME").unwrap_or_else(|_| "Reisebloggen".to_string());

    let email = Message::builder()
        .from(
            format!("{fra_navn} <{fra}>")
                .parse::<Mailbox>()
                .map_err(|e| e.to_string())?,
        )
        .to(format!("{epost} <{epost}>")
            .parse::<Mailbox>()
            .map_err(|e| e.to_string())?)
        .subject("Din innloggingskode")
        .multipart(
            MultiPart::alternative()
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_PLAIN)
                        .body(tekst.to_string()),
                )
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_HTML)
                        .body(html.to_string()),
                ),
        )
        .map_err(|e| e.to_string())?;

    let creds = lettre::transport::smtp::authentication::Credentials::new(
        std::env::var("SMTP_USER").unwrap_or_default(),
        std::env::var("SMTP_PASSWORD").unwrap_or_default(),
    );

    let mailer = AsyncSmtpTransport::<Tokio1Executor>::relay(host)
        .map_err(|e| e.to_string())?
        .credentials(creds)
        .build();

    mailer.send(email).await.map_err(|e| format!("{e:#?}"))?;

    Ok(())
}

fn logg_kode(epost: &str, kode: &str) {
    tracing::info!("Engangskode for {epost}: {kode} (utløper om 15 min)");
}
