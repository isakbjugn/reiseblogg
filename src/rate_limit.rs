//! Rate-limiting for offentlig trafikk mot auth-endepunktene.
//!
//! Bakgrunn: appen kjører bak Railways Envoy-edge. Offentlig trafikk får edge-satte
//! headere (`x-forwarded-for`, `x-real-ip`, `x-railway-edge`); intern trafikk over
//! `*.railway.internal` går utenom edgen og har dem ikke. En ekstern klient kan ikke
//! *fjerne* disse headerne, og kan heller ikke forfalske dem – testet mot prod:
//! en injisert `x-forwarded-for`/`x-real-ip` blir strippet og bygget på nytt av edgen.
//!
//! Derfor strupes KUN offentlig trafikk, nøklet på klientens `x-real-ip`. Intern
//! trafikk slippes urørt. Se diskusjonen i AUTH_PLAN.md i fagord-rust-api.
//!
//! MERK: dette antar at appen alltid står bak en proxy som setter disse headerne.
//! Kjøres den noen gang uten proxy, vil ALT klassifiseres som internt (og slippe
//! forbi). Deteksjons-headeren `x-forwarded-for` er valgt framfor Railway-spesifikke
//! `x-railway-edge` for portabilitet. I lokal utvikling fins ingen av dem, så
//! strupingen er en no-op.

use std::net::IpAddr;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use governor::clock::DefaultClock;
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};

/// Header hvis *tilstedeværelse* skiller offentlig fra intern trafikk. Settes av edgen
/// på all offentlig trafikk, mangler alltid internt. Kan overstyres via env for bytte
/// av hosting (f.eks. en annen proxy som bruker et annet navn).
fn edge_marker_header() -> String {
    std::env::var("RATE_LIMIT_EDGE_HEADER").unwrap_or_else(|_| "x-forwarded-for".to_string())
}

/// Header som bærer klientens sanne IP (ett rent ledd, i motsetning til `x-forwarded-for`
/// som har flere ledd der det høyre roterer mellom edge-noder). Brukes som strupenøkkel.
fn client_ip_header() -> String {
    std::env::var("RATE_LIMIT_IP_HEADER").unwrap_or_else(|_| "x-real-ip".to_string())
}

/// Keyed rate-limiter: én uavhengig bøtte per klient-IP.
pub type IpRateLimiter = RateLimiter<IpAddr, DefaultKeyedStateStore<IpAddr>, DefaultClock>;

/// Bygger limiteren: påfyll av én tillatelse hvert 12. sekund, med burst på inntil 5.
/// Altså ~5 forespørsler i rask rekkefølge, deretter én ny hvert 12. sekund per IP.
/// Verdiene er bevisst romslige – dette er volumdemping, ikke auth-forsvaret selv
/// (det er `MAX_ATTEMPTS` per kode + e-post-cooldown, som er IP-uavhengige).
pub fn build_limiter() -> Arc<IpRateLimiter> {
    let quota = Quota::with_period(Duration::from_secs(12))
        .expect("periode må være > 0")
        .allow_burst(NonZeroU32::new(5).expect("burst må være > 0"));
    Arc::new(RateLimiter::keyed(quota))
}

/// Starter en bakgrunnsoppgave som periodisk fjerner utløpte bøtter, så minnebruken
/// ikke vokser med antall unike IP-er over tid.
pub fn spawn_cleanup(limiter: Arc<IpRateLimiter>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(300));
        loop {
            interval.tick().await;
            limiter.retain_recent();
        }
    });
}

/// Resultatet av å klassifisere en forespørsel ut fra headerne. Skilt ut som ren
/// funksjon (uten `Request`/`Next`) så beslutningen kan enhetstestes direkte.
#[derive(Debug, PartialEq)]
pub enum Classification {
    /// Ingen edge-header: intern trafikk. Slippes forbi uten struping.
    Internal,
    /// Offentlig trafikk med gyldig klient-IP. Strupes på denne IP-en.
    External(IpAddr),
    /// Offentlig trafikk uten lesbar klient-IP. Uventet bak edgen – avvises.
    Malformed,
}

/// Avgjør hvordan en forespørsel skal behandles ut fra edge- og IP-headerne.
pub fn classify(headers: &HeaderMap) -> Classification {
    // Mangler edge-markøren -> intern trafikk.
    if !headers.contains_key(edge_marker_header().as_str()) {
        return Classification::Internal;
    }

    // Offentlig: hent den sanne klient-IP-en fra edge-satt header.
    match headers
        .get(client_ip_header().as_str())
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.trim().parse::<IpAddr>().ok())
    {
        Some(ip) => Classification::External(ip),
        None => Classification::Malformed,
    }
}

/// Axum-middleware som strupes offentlig auth-trafikk per klient-IP og slipper intern
/// trafikk forbi. Kobles på med `from_fn_with_state` (se `main.rs`).
pub async fn rate_limit_external(
    State(limiter): State<Arc<IpRateLimiter>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    match classify(req.headers()) {
        Classification::Internal => Ok(next.run(req).await),
        Classification::External(ip) => {
            if limiter.check_key(&ip).is_err() {
                tracing::warn!("Rate-limit nådd for offentlig klient {}", ip);
                return Err(StatusCode::TOO_MANY_REQUESTS);
            }
            Ok(next.run(req).await)
        }
        Classification::Malformed => {
            tracing::warn!("Offentlig forespørsel uten lesbar klient-IP – avvist");
            Err(StatusCode::BAD_REQUEST)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                value.parse().unwrap(),
            );
        }
        map
    }

    #[test]
    fn uten_edge_header_er_internt() {
        // Intern trafikk over railway.internal har verken x-forwarded-for eller x-real-ip.
        let h = headers(&[("host", "reiseblogg.railway.internal:8080")]);
        assert_eq!(classify(&h), Classification::Internal);
    }

    #[test]
    fn edge_header_med_gyldig_ip_er_eksternt() {
        let h = headers(&[
            ("x-forwarded-for", "185.127.100.1, 79.127.151.145"),
            ("x-real-ip", "185.127.100.1"),
        ]);
        let expected: IpAddr = "185.127.100.1".parse().unwrap();
        assert_eq!(classify(&h), Classification::External(expected));
    }

    #[test]
    fn edge_header_uten_lesbar_ip_er_malformed() {
        // Bak edgen forventer vi alltid x-real-ip; fravær er mistenkelig.
        let h = headers(&[("x-forwarded-for", "185.127.100.1")]);
        assert_eq!(classify(&h), Classification::Malformed);
    }

    #[test]
    fn edge_header_med_ugyldig_ip_er_malformed() {
        let h = headers(&[("x-forwarded-for", "185.127.100.1"), ("x-real-ip", "ikke-en-ip")]);
        assert_eq!(classify(&h), Classification::Malformed);
    }

    #[test]
    fn burst_slipper_gjennom_men_neste_avvises() {
        let limiter = build_limiter();
        let ip: IpAddr = "185.127.100.1".parse().unwrap();

        // Bøtta starter full: burst-antallet forespørsler skal slippe gjennom.
        // Testen kjører på mikrosekunder, så ingen nye tillatelser rekker å fylles på.
        for i in 1..=5 {
            assert!(
                limiter.check_key(&ip).is_ok(),
                "forespørsel {i} skulle vært tillatt (innenfor burst)"
            );
        }

        // Ingen tid har gått, så ingen påfyll: den neste skal strupes.
        assert!(
            limiter.check_key(&ip).is_err(),
            "forespørsel utover burst skulle vært avvist"
        );
    }

    #[test]
    fn ulike_ip_er_har_uavhengige_boetter() {
        let limiter = build_limiter();
        let a: IpAddr = "185.127.100.1".parse().unwrap();
        let b: IpAddr = "79.127.151.145".parse().unwrap();

        // Tøm bøtta til A.
        for _ in 0..5 {
            assert!(limiter.check_key(&a).is_ok());
        }
        assert!(limiter.check_key(&a).is_err(), "A skal være tømt");

        // B skal være helt upåvirket – det er hele poenget med keyed struping per IP.
        assert!(limiter.check_key(&b).is_ok(), "B skal ha egen full bøtte");
    }
}
