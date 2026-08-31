//! E-post: sender engangskoden for innlogging.
//!
//! Uten `RESEND_API_KEY` satt logges koden i stedet for å sendes.

use minijinja::{Environment, context};
use resend_rs::types::CreateEmailBaseOptions;
use resend_rs::{Resend, Result};

/// Sender engangskoden til e-postadressen, eller logger den hvis e-postutsending ikke er
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

    // I utvikling er RESEND_API_KEY usatt: logg koden framfor å sende.
    let Some(resend_api_key) = std::env::var("RESEND_API_KEY").ok() else {
        logg_kode(epost, kode);
        return;
    };

    send_email(&resend_api_key, epost, &html, &tekst).await.unwrap_or_else(|e| {
        tracing::error!("Kunne ikke sende e-post til {epost}: {e}");
        // Fallback så brukeren ikke står fast om e-postutsending ikke fungerer
        logg_kode(epost, kode);
    });
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

async fn send_email(resend_api_key: &str, user_email: &str, html: &str, text: &str) -> Result<()> {
    let resend = Resend::new(resend_api_key);

    let from = std::env::var("RESEND_FROM").unwrap_or_else(|_| "onboarding@resend.dev".into());
    let to = [user_email];
    let subject = "Din innloggingskode";

    let email = CreateEmailBaseOptions::new(from, to, subject)
      .with_html(html)
      .with_text(text);

    let _email = resend.emails.send(email).await?;
    println!("{:?}", _email);

    Ok(())
}

fn logg_kode(epost: &str, kode: &str) {
    tracing::info!("Engangskode for {epost}: {kode} (utløper om 15 min)");
}
