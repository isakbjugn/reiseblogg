# Plan: reiseblogg

Reiseblogg for to som skriver fra Sør-Amerika. Egenutviklet, ikke Wordpress.

**Arkitektur:** én Rust-app (Axum) som eier alt – database, auth, HTML-rendring og
bildeopplasting. Lesesidene er server-rendret HTML uten JavaScript. Editoren er en
Preact-island. Én tjeneste å deploye, ingen intern nettverkskonfigurasjon.

**Tilnærming:** samme rekkefølge som spiken – hvert steg skal kunne kjøres og ses i
nettleseren før neste begynner. Ingen halvferdige lag som venter på hverandre.

**Lesertilgang:** helt åpen. Søkemotorer indekserer, ingen gjesteinnlogging. Dere to
logger inn kun for å skrive.

---

## Status: hva spiken alt har bevist

Ferdig og verifisert i `reiseblogg-spike/`:

- minijinja med `path_loader` + `{% extends %}`-arv (`base.html` → 3 sider)
- Lesesider med **0 script-tagger** – server-rendret HTML
- Preact-island: `useState`/`useEffect`, htm-syntaks uten transpilering
- Live Markdown-forhåndsvisning, kladd i `localStorage` som overlever refresh
- Vendrede avhengigheter i `static/vendor/`, **0 forespørsler til esm.sh**
- Brotli via `CompressionLayer` (markdown-it 147 kB → 44 kB)
- `cargo watch -w src/ -w templates/` som hele dev-oppsettet

---

## Steg 0: Spike → prosjekt

Flytt `~/reiseblogg-spike` → `~/git/rust/reiseblogg`, `git init`, første commit.

- Døp pakken `reiseblogg` i `Cargo.toml`
- `.gitignore`: `/target`, `.env`, `db_persistent_storage/`
- **Commit `static/vendor/` til git.** Det er poenget med å vendre – filene skal
  ikke hentes ved bygg.
- `CLAUDE.md` etter mønster fra fagord-rust-api: stack, kommandoer, struktur,
  utviklingsprinsipper (læringsprosjekt, bevisst bruk av abstraksjoner)

**Ferdig når:** `cargo watch` kjører fra ny plassering, alt fungerer som før.

---

## Steg 1: Ren Rust + templates + Preact

Ferdig sidestruktur *før* databasen, med hardkodede poster. Da er alle
template-spørsmål avklart mens de er billige å endre.

- **`pulldown-cmark` (0.13)** for Markdown → HTML i Rust
  - **Rå HTML AV.** `post.html` bruker `| safe`, så HTML-passering er en direkte
    XSS-vei. Samme innstilling som `markdown-it` (`html: false`) i klienten.
- Sider: forside (postliste), enkeltpost, arkiv, om-oss, 404
- Editor: som i spiken, men også `/rediger/{slug}`-varianten
- Design: hent uttrykket fra `adrian-og-celine` (Playfair Display + Lato, dempet
  bakgrunn, `<details>` framfor JS). **Selvhostede fontfiler**, ikke Google Fonts –
  samme argument som for vendor-filene, og raskere på dårlig nett.
- `<meta>`-tagger + Open Graph, så delte lenker ser riktige ut i chat og sosiale medier
- `favicon.ico` (404-en fra spiken)

**Ferdig når:** hele nettstedet kan navigeres med hardkodede poster og ser ferdig ut.

---

## Steg 2: Database

PostgreSQL + SQLx med kompileringstidsvalidering, som fagord-rust-api.

**Merk:** fagord-rust-api bruker SQLx 0.8.6; 0.9.0 er ute. Velg én versjon og hold
den – blandede versjoner mellom prosjektene gir forvirrende `.sqlx`-cache-feil.

### Tabeller

| Tabell | Felt |
|---|---|
| `author` | `id, email` (unik på `lower(email)`), `name` |
| `post` | `id, slug` (unik), `title, content` (Markdown), `published_at` (nullable), `created_by, updated_by, created_at, updated_at` |

- Gjenbruk `set_updated_at()`-triggeren fra fagord-rust-api
- `published_at IS NULL` = kladd. Lar deg skrive på bussen og publisere når nettet kommer.
- Slug genereres fra tittel ved oppretting, **låst etterpå** – lenker skal ikke brytes
- `created_by` er `NOT NULL` uten `ON DELETE` (RESTRICT), som `article` i fagord-rust-api
- Dere to legges inn manuelt med `psql`. **Ingen registreringsflyt** – to brukere.

### Kode

- `src/db/post.rs`, `src/types/post.rs` – mønster fra `db/article.rs`
- Skill DB-type fra visnings-type, så `created_by` ikke lekker ut
- Forside sortert på `published_at DESC`, kladd kun synlig for innlogget forfatter

**Ferdig når:** postene kommer fra databasen. Fortsatt ingen innlogging – bruk en
midlertidig hardkodet forfatter-id.

---

## Steg 3: Auth

Hentes fra `fagord-rust-api` – dette er den mest gjennomtenkte koden du har.

### Kopier ~uendret

`src/auth.rs`, `src/tokens.rs`, `src/extract.rs` (`AuthenticatedAuthor`,
`HasDb`-traiten), `src/db/magic_token.rs`, `src/db/session.rs`, `src/rate_limit.rs`,
migrasjonene for `magic_tokens` og `session`.

Magic-kode framfor passord: ingenting å glemme på reise.

### Tilpass til monolitt

Dette er den ene reelle forenklingen ved å slå sammen:

- **Cookie i stedet for Bearer-token.** Ingen React Router-server i midten som må
  bære et token den ikke kan validere. `SESSION_SECRET` forsvinner – sesjonstokenet
  er 256 bit tilfeldighet verifisert mot `session`-tabellen og kan ikke forfalskes.
- Cookie: `HttpOnly`, `Secure`, `SameSite=Lax`, `Path=/`
- Extractor leser cookie framfor `Authorization`-header
- **CSRF:** `SameSite=Lax` dekker skjema-POST fra andre nettsteder. Verifiser at
  ingen skrive-endepunkter svarer på `GET`.

### E-post

Herfra kommer den andre halvdelen: `fagord-rust-api` har
`// TODO (byggesteg 6): send koden på e-post i stedet for å logge den`.

- `src/email.rs` sender via **Resend over HTTPS** (`resend-rs`), ikke SMTP/lettre.
  Railway blokkerer utgående SMTP (25/465/587) på Hobby-planen; HTTPS er tillatt.
- Minijinja-rendereren (`templates/epost/magic-kode.html`) gjenbrukes, mønsteret ellers fra `rust-auth`
- **Ikke** Argon2/PASETO/`confirmation_token.rs` – annen auth-modell, eldre

**Ferdig når:** du får engangskode på e-post, logger inn, og ser kladdene dine.

---

## Steg 4: Publisering, redigering, sletting

Autorisasjonen forenkles: enhver innlogget forfatter kan endre og slette alt innhold.

- `POST /ny`, `POST /rediger/{slug}`, `POST /slett/{slug}`
- Regel: **enhver innlogget forfatter**. `404` hvis slug mangler.
- `actions`-mønsteret: templaten får `["endre", "slett"]` og vet hvilke knapper å vise
- Publiser/avpubliser setter `published_at`
- **Progressive enhancement:** vanlige `<form method="post">`, så publisering fungerer
  selv om JS feiler. Editoren forbedrer, den er ikke et krav.

**Ferdig når:** hele skriveflyten fungerer ende-til-ende.

---

## Steg 5: Bilder og film

Det eneste reelle nybygget.

### Cloudflare R2, ikke AWS S3

S3-kompatibelt API (`aws-sdk-s3` 1.140 fungerer rett mot det), men **null
egress-kostnad**. På en bildetung blogg som familie skal bla i, er det forskjellen
mellom gratis og en uventet regning. CDN i front som standard – og *her* er
edge-argumentet ditt riktig: bildene er megabytes til lesere i Norge.

### Presignert opplasting

Nettleseren laster opp **direkte til R2**, ikke gjennom Rust. En 40 MB video gjennom
backend betyr dobbel overføring og timeout på dårlig nett.

- `POST /media` → krever innlogging, validerer filtype + størrelse, returnerer
  presignert PUT-URL + endelig offentlig URL
- `media`-tabell: `id, post_id (nullable), key, mime_type, uploaded_by, created_at`.
  Uten den har du ingen måte å finne foreldreløse filer.

### Klientside før opplasting

- **Skalering i `<canvas>`.** Et telefonbilde er 5 MB; maks 2000 px sparer både
  opplastingstid der du er og nedlastingstid for leserne.
- **EXIF/GPS strippes** som bieffekt av canvas-reskalering. Verdt å merke seg:
  telefonbilder inneholder koordinater du sannsynligvis ikke vil publisere.
- Videoer kan ikke skaleres i nettleseren – sett en størrelsesgrense og last opp rått
- Editoren: dra-og-slipp, sett inn `![](url)` i Markdown ved fullført opplasting
- `loading="lazy"` + `width`/`height` på bilder, så lesesidene ikke hopper

**Ferdig når:** du kan dra et telefonbilde inn i editoren og se det i publisert post.

---

## Steg 6: Deploy

- **Dockerfile:** flerstegs. `cargo build --release` (profilen finnes:
  `strip`/`lto`/`opt-level = "s"`), så kopier binæret + `templates/` + `static/`.
  **Ingen Node** – vendor-filene ligger i git.
- Railway: én tjeneste + Postgres. Env: `DATABASE_URL`, `RESEND_API_KEY` + `RESEND_FROM`, R2-nøkler.
- `sqlx migrate run` ved oppstart eller som eget steg
- Eget domene + HTTPS
- Legg inn dere to som forfattere i prod med `psql`
- **Backup av databasen.** Ta en dump før avreise og periodisk underveis – teksten
  er det eneste som ikke finnes noe annet sted. Bildene ligger i R2.

**Ferdig når:** bloggen er live og du kan publisere fra telefonen.

---

## Etter avreise, hvis det trengs

- **WASM-kompilert `pulldown-cmark`** – fjerner de to renderene og markdown-its 44 kB.
  Størst gevinst av det som står her, men ikke blokkerende.
- Kommentarer fra lesere (krever spam-håndtering – ikke undervurder det)
- RSS-feed
- Kart med reiseruten
- Bildegalleri som Preact-island
- Forfatternavn i kladd-listen (hvem opprettet, hvem endret sist) – med delte kladder
  er det nyttig å se eierskap. Ett `forfatter`-felt i `KladdRad` + join i
  `db_get_kladder` + visning i `forside.html`.

---

## Bevisste utelatelser

- **Ingen registreringsflyt.** To brukere, `psql` er raskere.
- **Ingen roller.** To forfattere med lik tilgang.
- **Ingen tester i steg 1–2.** Kommer i steg 3–4, der auth og autorisasjon faktisk
  har regler verdt å teste. `tests/auth.rs` og `tests/articles.rs` fra fagord-rust-api
  er utgangspunktet.
- **Ingen i18n.** Norsk.
- **Ingen kommentarer i første versjon.**

## Risiko

| Risiko | Håndtering |
|---|---|
| Dårlig nett i Sør-Amerika | Kladd i `localStorage`, klientskalering, `published_at` for å skrive offline-ish |
| To Markdown-renderere divergerer | Rå HTML av i begge. WASM fjerner problemet permanent. |
| Databasen mistes | Dump før avreise + periodisk |
| Tiden før avreise | Steg 1–4 gir en fungerende blogg. Bilder (steg 5) kan komme etter at dere har reist – tekst først. |
