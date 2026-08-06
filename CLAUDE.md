# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Om prosjektet

Reiseblogg for to som skriver fra Sør-Amerika. Egenutviklet framfor Wordpress.

Én Rust-app eier alt: database, auth, HTML-rendring og bildeopplasting. Lesesidene er
server-rendret HTML uten JavaScript. Editoren er en Preact-island. Én tjeneste å deploye.

`PLAN.md` beskriver stegene fram til ferdig blogg og hva som bevisst er utelatt.

## Teknisk stack

- **Rammeverk**: Axum 0.8
- **Templates**: minijinja 2 med `path_loader`
- **Markdown**: pulldown-cmark
- **Klientside**: Preact + htm, uten byggekjede
- **Runtime**: Tokio

Kommer senere: PostgreSQL med SQLx (steg 2), Cloudflare R2 (steg 5).

### Teknologivalg

**Monolitt framfor delt frontend/backend.** Fagord har et Rust-API og en separat React
Router-app fordi API-et er offentlig og har selvstendig verdi. Bloggen har én konsument,
for alltid – da gir splitten bare to deploys, intern nettverkskonfigurasjon og to sett
hemmeligheter uten gevinst. Sammenslåingen fjerner også et helt sesjonslag: ingen
`SESSION_SECRET`, fordi ingen mellomtjener må bære et token den ikke kan validere.

**minijinja framfor askama/rinja.** `path_loader` leser templates fra disk ved kjøring,
så HTML holdes i `.html`-filer med syntaksfarging og kan redigeres uten å røre Rust-koden.
Prisen er at templates ikke er typesjekket.

**Preact + htm framfor React + Vite.** `htm` gir JSX-lignende syntaks via tagged
templates, som nettleseren forstår direkte – ingen transpilering, ingen byggekjede,
ingen Node i Dockerfilen. Hooks-API-et er identisk med React. Preact-stacken er ~6,6 kB
brotli mot Reacts ~45 kB.

**Islands framfor SPA.** Lesesidene laster 0 script-tagger. Bare editoren trenger
JavaScript, og den monteres i én `<div>`.

**Vendrede avhengigheter.** `static/vendor/` er committet til git. Ingen forespørsler til
esm.sh ved kjøring: bloggen skal fungere fra nettverk som filtrerer tredjepartsdomener,
og bestå etter reisen. Filene er hentet med `?bundle`, som slår transitive avhengigheter
sammen (markdown-it alene var 22 forespørsler direkte fra esm.sh).

## Kommandoer

```bash
cargo watch -q -c -w src/ -w templates/ -x run   # Utvikling
cargo run                                        # Kjør serveren
cargo build --release                            # Produksjonsbygg
cargo clippy                                     # Lint
cargo fmt                                        # Formatering (max_width = 120)
```

Serveren lytter på http://127.0.0.1:8080.

`-w templates/` er ikke pynt: minijinja cacher hver template etter første innlasting, så
en redigert `.html` får ingen effekt i en kjørende prosess. `cargo watch` løser det ved å
restarte, framfor at koden må tømme cachen selv.

`static/` trenger **ikke** overvåkes – `ServeDir` leser fra disk per forespørsel, så
endringer i CSS og `editor.js` krever bare refresh.

## Prosjektstruktur

```
src/
├── main.rs          # Ruter, AppState, serveroppsett
├── markdown.rs      # Markdown → HTML (pulldown-cmark, rå HTML AV)
└── poster.rs        # Handlere for lesesidene + editoren
templates/
├── base.html        # Felles skall – INGEN <script> her
├── forside.html     # Postliste
├── post.html        # Enkeltpost
├── arkiv.html       # Alle poster gruppert på måned
├── om-oss.html
├── editor.html      # Import map + Preact-islanden
└── 404.html
static/
├── stil.css
├── editor.js        # Preact + htm
└── vendor/          # preact, preact/hooks, htm, markdown-it (committet)
```

## Sikkerhet

**Rå HTML i Markdown er AV i begge renderere.** `post.html` bruker `| safe` på innholdet
(det er ferdig rendret HTML), så en Markdown-renderer med HTML-passering på er en direkte
XSS-vei. Gjelder både `pulldown-cmark` i Rust og `markdown-it` i klienten (`html: false`).

**To renderere kan divergere.** Klienten viser forhåndsvisningen, Rust rendrer det
publiserte. Begge kjører CommonMark uten rå HTML, men de er ikke garantert identiske.
Alternativet er `pulldown-cmark` kompilert til WASM – én renderer. Se `PLAN.md`.

## Utviklingsprinsipper

### Læringsprosjekt

Dette er et læringsprosjekt. Nye løsninger bør implementeres på måter som gir innsikt i
Rust og webutvikling – forklar gjerne underliggende konsepter og alternativer.

### HTML først

Foretrekk HTML og CSS framfor JavaScript der det er mulig. Nettleserne har mye innebygd
funksjonalitet som ofte er bedre enn JavaScript-løsninger. Skriveflyten skal fungere med
vanlige `<form method="post">` selv om JS feiler – editoren forbedrer, den er ikke et krav.

### Bevisst bruk av abstraksjoner

Foretrekk løsninger som viser hva som skjer framfor å skjule kompleksitet. Vurder alltid
om et problem kan løses med egen kode før du tar inn et bibliotek.
