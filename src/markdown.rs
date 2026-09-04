//! Markdown → HTML.
//!
//! Dette er den ene renderen som produserer det leserne faktisk får. Editoren har
//! sin egen (markdown-it i nettleseren) for forhåndsvisningen, og de to er ikke
//! garantert identiske – begge kjører CommonMark uten rå HTML, men det er en
//! bevisst avveining, ikke en garanti. Se `PLAN.md` om WASM-alternativet, som
//! ville gjort dette til den *eneste* renderen.
//!
//! SIKKERHET: rå HTML passeres ikke videre. `post.html` setter inn resultatet med
//! `| safe`, så en renderer som slipper gjennom `<script>` ville vært en direkte
//! XSS-vei. `pulldown-cmark` har rå HTML *på* som standard, så det må slås av
//! eksplisitt – i motsetning til markdown-it, der `html: false` er standard.

use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, html};

/// Aktiverte utvidelser utover CommonMark.
///
/// Bevisst utenfor: `ENABLE_MATH` (ingen formler i et reisebrev) og
/// `ENABLE_OLD_FOOTNOTES` (erstattet av `ENABLE_FOOTNOTES`).
fn options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH); // ~~utstrøket~~
    options.insert(Options::ENABLE_TABLES); // tabeller
    options.insert(Options::ENABLE_FOOTNOTES); // [^1]
    options.insert(Options::ENABLE_SMART_PUNCTUATION); // -- → –, ... → …
    options
}

/// URL-skjemaer som kan kjøre kode i nettleseren.
///
/// `pulldown-cmark` slipper disse gjennom urørt – verifisert, ikke antatt: en test
/// på `[a](javascript:alert(1))` feilet før denne filtreringen fantes. Merk at
/// markdown-it i klienten blokkerer dem som standard, så Rust-siden var den
/// svakeste av de to.
const FARLIGE_SKJEMAER: [&str; 3] = ["javascript:", "vbscript:", "data:"];

/// Sant hvis URL-en bruker et skjema som kan kjøre kode.
///
/// Sammenligner små bokstaver (`JaVaScRiPt:` slipper ellers forbi) og trimmer
/// innledende blanktegn, som nettleserne ignorerer. Vi trimmer ikke tegn *inne* i
/// skjemaet, fordi `java\tscript:` allerede stopper i Markdown-parseren – lenken
/// blir ikke gjenkjent i det hele tatt.
fn er_farlig_url(url: &str) -> bool {
    let normalisert = url.trim_start().to_ascii_lowercase();
    FARLIGE_SKJEMAER.iter().any(|skjema| normalisert.starts_with(skjema))
}

/// Rendrer Markdown til HTML, med rå HTML og kodekjørende URL-er fjernet.
///
/// To ting skjer:
///
/// 1. `Event::Html`/`Event::InlineHtml` filtreres bort framfor å escapes: skriver
///    noen `<script>` i en post, forsvinner det – det vises ikke som synlig tekst.
///    Riktig for en blogg der vi selv skriver innholdet; vi vil oppdage at noe
///    mangler, ikke at HTML-en vår står i klartekst.
/// 2. `javascript:`-lenker og -bilder nulles til `#`. Se `er_farlig_url`.
pub fn til_html(markdown: &str) -> String {
    let parser = Parser::new_ext(markdown, options())
        .filter(|event| !matches!(event, Event::Html(_) | Event::InlineHtml(_)))
        .map(|event| match event {
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) if er_farlig_url(&dest_url) => Event::Start(Tag::Link {
                link_type,
                dest_url: CowStr::Borrowed("#"),
                title,
                id,
            }),
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) if er_farlig_url(&dest_url) => Event::Start(Tag::Image {
                link_type,
                dest_url: CowStr::Borrowed(""),
                title,
                id,
            }),
            annet => annet,
        });

    // Litt større enn kilden: HTML-taggene kommer i tillegg til teksten.
    let mut html_ut = String::with_capacity(markdown.len() * 3 / 2);
    html::push_html(&mut html_ut, parser);

    // `loading="lazy"` på alle bilder, så nettleseren utsetter bilder utenfor
    // skjermen til leseren ruller nær dem – viktig på en bildetung blogg lest på
    // dårlig nett. pulldown-cmark har ingen støtte for dette: `Tag::Image` bærer
    // bare `link_type`, `dest_url`, `title` og `id`, og det eneste attributt-
    // tillegget (`ENABLE_HEADING_ATTRIBUTES`) gjelder overskrifter, ikke bilder.
    // Strengerstatningen er presis og trygg: rendereren skriver alltid `<img src="`
    // som åpning på bilde-tagger, rå HTML er filtrert bort ovenfor, og `<`/`>` i
    // tekst er escapet – så sekvensen kan bare komme fra en ekte bilde-tag.
    html_ut.replace("<img src=\"", "<img loading=\"lazy\" src=\"")
}

/// Første avsnitt som ren tekst, til `<meta name="description">` og Open Graph.
///
/// Tar bare tekst-eventene, så formatering og bilder faller bort. Klipper på
/// ordgrense for å unngå å kutte midt i et ord.
pub fn sammendrag(markdown: &str, maks_tegn: usize) -> String {
    let mut tekst = String::new();

    for event in Parser::new_ext(markdown, options()) {
        match event {
            Event::Text(t) | Event::Code(t) => tekst.push_str(&t),
            // Avsnittslutt: har vi noe, er vi ferdige – vi vil bare det første.
            Event::End(pulldown_cmark::TagEnd::Paragraph) if !tekst.trim().is_empty() => break,
            Event::SoftBreak | Event::HardBreak => tekst.push(' '),
            _ => {}
        }
    }

    let tekst = tekst.trim();

    if tekst.chars().count() <= maks_tegn {
        return tekst.to_string();
    }

    // Klipp på siste mellomrom innenfor grensen, så vi ikke deler et ord.
    let klippet: String = tekst.chars().take(maks_tegn).collect();
    let kort = match klippet.rfind(' ') {
        Some(i) => &klippet[..i],
        None => &klippet,
    };

    format!("{}…", kort.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendrer_vanlig_markdown() {
        let html = til_html("# Tittel\n\nTekst med **fet** og *kursiv*.");
        assert!(html.contains("<h1>Tittel</h1>"));
        assert!(html.contains("<strong>fet</strong>"));
        assert!(html.contains("<em>kursiv</em>"));
    }

    #[test]
    fn rendrer_lenker_og_bilder() {
        let html = til_html("[lenke](https://example.com) og ![bilde](/b.jpg)");
        assert!(html.contains(r#"<a href="https://example.com">lenke</a>"#));
        assert!(html.contains(r#"<img loading="lazy" src="/b.jpg" alt="bilde""#));
    }

    #[test]
    fn bilder_faar_lazy_loading() {
        let html = til_html("![bilde](/b.jpg)");
        assert!(html.contains(r#"<img loading="lazy" src="/b.jpg" alt="bilde""#));
    }

    #[test]
    fn aktiverte_utvidelser() {
        assert!(til_html("~~vekk~~").contains("<del>vekk</del>"));
        assert!(til_html("| a | b |\n|---|---|\n| 1 | 2 |").contains("<table>"));
        // Smart tegnsetting: to bindestreker blir tankestrek.
        assert!(til_html("så -- altså").contains('–'));
    }

    // --- Sikkerhet: rå HTML skal ikke overleve ---

    #[test]
    fn fjerner_script() {
        let html = til_html("før\n\n<script>alert(1)</script>\n\netter");
        assert!(!html.contains("<script"));
        assert!(!html.contains("alert(1)"));
        assert!(html.contains("før"));
        assert!(html.contains("etter"));
    }

    #[test]
    fn fjerner_inline_html_og_hendelseshandterere() {
        let html = til_html(r#"tekst <img src=x onerror="alert(1)"> mer"#);
        assert!(!html.contains("<img"));
        assert!(!html.contains("onerror"));
        assert!(html.contains("tekst"));
    }

    #[test]
    fn fjerner_iframe_og_style() {
        let html = til_html("<iframe src=\"//evil\"></iframe>\n\n<style>body{display:none}</style>");
        assert!(!html.contains("<iframe"));
        assert!(!html.contains("<style"));
    }

    #[test]
    fn nuller_kodekjorende_lenker() {
        // pulldown-cmark slipper disse gjennom urørt – denne testen feilet før
        // `er_farlig_url` fantes. XSS-vei som ikke går via rå HTML i det hele tatt.
        for md in [
            "[klikk](javascript:alert(1))",
            "[klikk](JaVaScRiPt:alert(1))",
            "[klikk](  javascript:alert(1))",
            "[klikk](vbscript:msgbox(1))",
            "[klikk](data:text/html,noe)",
        ] {
            let html = til_html(md);
            assert!(
                !html.to_ascii_lowercase().contains("javascript:"),
                "slapp gjennom: {md}"
            );
            assert!(!html.to_ascii_lowercase().contains("vbscript:"), "slapp gjennom: {md}");
            assert!(!html.to_ascii_lowercase().contains("data:"), "slapp gjennom: {md}");
            // Lenketeksten skal bestå – bare målet nulles.
            assert!(html.contains("klikk"));
        }
    }

    #[test]
    fn nuller_kodekjorende_bilder() {
        let html = til_html("![b](javascript:alert(1))");
        assert!(!html.contains("javascript:"));
    }

    #[test]
    fn beholder_trygge_urler() {
        assert!(til_html("[a](https://ok.example)").contains(r#"href="https://ok.example""#));
        assert!(til_html("[a](/arkiv)").contains(r#"href="/arkiv""#));
        assert!(til_html("[a](mailto:x@y.no)").contains("mailto:x@y.no"));
        // Bildefiler er hele poenget med en reiseblogg.
        assert!(til_html("![b](/static/bilde.jpg)").contains(r#"src="/static/bilde.jpg""#));
    }

    #[test]
    fn kodeblokk_escapes_ikke_tolkes() {
        // Inne i en kodeblokk skal HTML vises som tekst, ikke fjernes.
        let html = til_html("```\n<script>x</script>\n```");
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<script>"));
    }

    // --- Sammendrag ---

    #[test]
    fn sammendrag_tar_forste_avsnitt() {
        let s = sammendrag("Første avsnitt her.\n\nAndre avsnitt.", 200);
        assert_eq!(s, "Første avsnitt her.");
    }

    #[test]
    fn sammendrag_hopper_over_overskrift() {
        let s = sammendrag("# Overskrift\n\nBrødteksten starter her.", 200);
        assert!(s.contains("Brødteksten"));
    }

    #[test]
    fn sammendrag_stripper_formatering() {
        let s = sammendrag("Tekst med **fet** og [lenke](/a).", 200);
        assert_eq!(s, "Tekst med fet og lenke.");
    }

    #[test]
    fn sammendrag_klipper_pa_ordgrense() {
        let s = sammendrag("ett to tre fire fem seks sju åtte ni ti", 15);
        assert!(s.ends_with('…'));
        assert!(!s.contains("  "));
        // Skal ikke dele et ord: siste tegn før … er ikke midt i et ord.
        assert!(s.starts_with("ett to tre"));
    }

    #[test]
    fn sammendrag_takler_tom_og_kort_tekst() {
        assert_eq!(sammendrag("", 100), "");
        assert_eq!(sammendrag("Kort.", 100), "Kort.");
    }

    #[test]
    fn sammendrag_takler_flerbyte_tegn_pa_grensen() {
        // Norske tegn er 2 byte i UTF-8. Klipping må skje på char-grense, ellers
        // panikker slicing. Grensen settes midt i «høydesyke».
        let s = sammendrag("Cusco og høydesyke er slitsomt å oppleve", 12);
        assert!(s.ends_with('…'));
    }
}
