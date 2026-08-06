# reiseblogg-spike

Spike som viser at **Rust + minijinja + Preact-island** henger sammen, uten byggekjede
og uten Node i drift. Ingen database, ingen faktiske bloggposter – tre falske poster
ligger hardkodet i `main.rs`.

## Hva spiken beviser

- Lesesidene rendres av Rust som ren HTML – **0 script-tagger**, indekserbart, fungerer uten JS.
- Editoren er en **Preact-island**: én `<div>` som Preact monterer seg i, resten er server-HTML.
- **Ingen byggekjede.** Nettleseren laster ES-moduler direkte; `htm` gir JSX-lignende
  syntaks via tagged templates, så ingen transpilering trengs.
- Avhengighetene hostes selv fra `static/vendor/` – ingen forespørsler til esm.sh.

## Utvikling

```bash
cargo watch -q -c -w src/ -w templates/ -x run
```

Serveren lytter på http://127.0.0.1:8080.

`-w templates/` er ikke pynt: minijinja cacher hver template etter første innlasting,
så en redigert `.html` får ingen effekt i en kjørende prosess. `cargo watch` løser det
ved å restarte, framfor at koden må tømme cachen selv.

`static/` trenger **ikke** overvåkes – `ServeDir` leser fra disk per forespørsel, så
endringer i `stil.css` og `editor.js` krever bare refresh i nettleseren.

## Produksjon

```bash
cargo build --release
./target/release/reiseblogg-spike
```

Release-bygget bruker `strip`/`lto`/`opt-level = "s"` (samme profil som fagord-rust-api).
Templates leses fra `templates/` relativt til arbeidskatalogen, så den mappa – og
`static/` – må ligge ved siden av binæret.

## Ruter

| Rute | Innhold |
|---|---|
| `/` | Postliste, ren HTML |
| `/post/{slug}` | Enkeltpost, ren HTML. Slugs: `cusco-og-hoydesyke`, `buss-gjennom-atacama`, `forste-dag-i-bogota` |
| `/ny` | Editor med live Markdown-forhåndsvisning (den ene siden som laster JS) |

## Avhengigheter i nettleseren

Hostet selv fra `static/vendor/`, pekt på via import map i `templates/editor.html`:

| Fil | Rå | Brotli |
|---|---|---|
| `preact.js` | 11 kB | 4,4 kB |
| `preact-hooks.js` | 3,8 kB | 1,5 kB |
| `htm-preact.js` | 1,4 kB | 0,7 kB |
| `markdown-it.js` | 147 kB | 44 kB |

Filene er hentet fra esm.sh med `?bundle`, som slår transitive avhengigheter sammen –
markdown-it alene trakk 22 forespørsler direkte fra esm.sh, men er én fil her.
`CompressionLayer` gjør brotli-tallene til det som faktisk sendes.

Vil du laste fra esm.sh i stedet, er byttet **kun de fire stiene** i import map-en;
URL-ene ligger i en kommentar i `editor.html`. Ingen annen kode berøres.

## Kjente hull (bevisst utenfor spiken)

- Ingen database – postene er hardkodet.
- Ingen auth. Kommer fra `fagord-rust-api` (magic-kode-flyt, sesjoner, rate limiting).
- Ingen publisering. Editoren lagrer kladd i `localStorage`, men sender ingenting.
- Ingen bildeopplasting (R2 + presignert PUT).
- **To Markdown-renderere.** `markdown-it` i klienten, `pulldown-cmark` i Rust senere.
  Begge må kjøre med rå HTML *av*: `post.html` bruker `| safe` på innholdet, så en
  renderer med HTML på er en direkte XSS-vei. Alternativet er `pulldown-cmark`
  kompilert til WASM – én renderer, garantert identisk forhåndsvisning.
- `/favicon.ico` gir 404.
