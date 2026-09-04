// Editoren som Preact-island.
//
// Ingen byggekjede: nettleseren laster dette som en ES-modul, og import map-en i
// editor.html løser modulnavnene mot /static/vendor. `htm` gir oss JSX-lignende
// syntaks via tagged templates, som nettleseren forstår uten transpilering.
//
// Hooks-API-et er identisk med React, så koden fra temasider.ny.tsx i fagord
// går nesten ordrett over – bare `class` i stedet for `className`, og `onInput`
// i stedet for `onChange`.

import { render } from 'preact';
import { useEffect, useMemo, useRef, useState } from 'preact/hooks';
import { html } from 'htm/preact';
import MarkdownIt from 'markdown-it';

// Én nøkkel per post, slik at en kladd på «Cusco» ikke overskriver en kladd på
// «Atacama». Ny post får nøkkelen «ny».
const kladdNøkkel = (slug) => `reiseblogg:kladd:${slug || 'ny'}`;

// `html: false` slår av rå HTML i Markdown. Viktig: forhåndsvisningen settes inn
// med dangerouslySetInnerHTML, og serveren rendrer det publiserte med
// pulldown-cmark på samme innstilling – ellers divergerer de to. Se CLAUDE.md.
const md = new MarkdownIt({ html: false, linkify: true, breaks: false });

// Bildebehandling: maks dimensjon og JPEG-kvalitet. 2000 px er rikelig for en
// nettside og kutter telefonbilder (5 MB+) ned til noen få hundre kB. EXIF/GPS
// strippes som bieffekt – canvas holder kun pikseldata, ikke metadata.
const MAKS_PIKSEL = 2000;
const JPEG_KVALITET = 0.82;

// Formater som kan bære gjennomsiktighet beholdes – JPEG støtter ikke alpha, så
// et PNG-bilde kodet om til JPEG ville fått svart/hvit bakgrunn. Foto (JPEG/HEIC)
// kodes om til JPEG. GIF normaliseres til PNG (full alpha framfor 1-bits).
function utFormat(fileType) {
  if (fileType === 'image/png' || fileType === 'image/gif') return 'image/png';
  if (fileType === 'image/webp') return 'image/webp';
  return 'image/jpeg';
}

// Skalerer et bilde ned til maks 2000 px og koder det om – til JPEG for foto,
// men til et gjennomsiktighetsstøttende format for PNG/WebP. `from-image` får
// nettleseren til å respektere EXIF-orienteringen, så portrettbilder ikke havner
// rotert. HEIC er Safari-spesifikt å dekode: på andre nettlesere feiler
// createImageBitmap, og vi ber heller brukeren konvertere.
async function skalerBilde(file) {
  let bitmap;
  try {
    bitmap = await createImageBitmap(file, { imageOrientation: 'from-image' });
  } catch {
    throw new Error('Kunne ikke lese bildet – konverter HEIC til JPEG først.');
  }

  const skala = Math.min(1, MAKS_PIKSEL / Math.max(bitmap.width, bitmap.height));
  const bredde = Math.round(bitmap.width * skala);
  const høyde = Math.round(bitmap.height * skala);

  const canvas = document.createElement('canvas');
  canvas.width = bredde;
  canvas.height = høyde;
  canvas.getContext('2d').drawImage(bitmap, 0, 0, bredde, høyde);
  bitmap.close();

  const mime_type = utFormat(file.type);

  const blob = await new Promise((resolve, reject) => {
    canvas.toBlob(
      (b) => (b ? resolve(b) : reject(new Error('Kunne ikke kode om bildet'))),
      mime_type,
      JPEG_KVALITET, // ignorert for PNG (alltid lossless)
    );
  });
  return { blob, mime_type };
}

function Editor({ slug, handling, avbryt, startTittel, startInnhold, publisert }) {
  // Kladd fra localStorage vinner over serverens startverdi: mistet nett eller
  // et uhell med refresh skal ikke spise det du har skrevet. Kritisk på reise.
  const kladd = useMemo(() => {
    try {
      return JSON.parse(localStorage.getItem(kladdNøkkel(slug)) ?? 'null');
    } catch {
      return null;
    }
  }, [slug]);

  const [tittel, setTittel] = useState(kladd?.tittel ?? startTittel);
  const [innhold, setInnhold] = useState(kladd?.innhold ?? startInnhold);
  const [opplasting, setOpplasting] = useState(false);
  const [opplastingsFeil, setOpplastingsFeil] = useState('');
  const [drarOver, setDrarOver] = useState(false);
  const tekstfelt = useRef(null);
  const filInput = useRef(null);

  useEffect(() => {
    localStorage.setItem(kladdNøkkel(slug), JSON.stringify({ tittel, innhold }));
  }, [slug, tittel, innhold]);

  const forhåndsvist = useMemo(() => md.render(innhold), [innhold]);

  // Bare relevant når kladden faktisk avviker fra det serveren sendte – ellers
  // er det ingenting å forkaste.
  const harUlagretKladd = tittel !== startTittel || innhold !== startInnhold;

  function forkast() {
    localStorage.removeItem(kladdNøkkel(slug));
    setTittel(startTittel);
    setInnhold(startInnhold);
  }

  // Setter inn tekst ved markøren, eller på slutten hvis feltet ikke har fokus.
  // Leser textarea-ens `value` direkte i stedet for `innhold`-staten, så tegn
  // brukeren rakk å skrive mens bildet ble prosessert ikke overskrives.
  function settInnMarkdown(tekst) {
    const el = tekstfelt.current;
    if (!el) {
      setInnhold((tidligere) => tidligere + tekst);
      return;
    }
    const start = el.selectionStart ?? el.value.length;
    const slutt = el.selectionEnd ?? el.value.length;
    setInnhold(el.value.slice(0, start) + tekst + el.value.slice(slutt));
    requestAnimationFrame(() => {
      el.focus();
      const pos = start + tekst.length;
      el.setSelectionRange(pos, pos);
    });
  }

  async function lastOpp(fil) {
    if (opplasting) return;
    if (!fil.type.startsWith('image/')) {
      setOpplastingsFeil('Kun bilder støttes foreløpig – video kommer senere.');
      return;
    }

    setOpplasting(true);
    setOpplastingsFeil('');
    try {
      const { blob, mime_type } = await skalerBilde(fil);

      const svar = await fetch('/media', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ mime_type, size: blob.size }),
      });
      if (svar.status === 401) {
        window.location.href = '/logg-inn';
        return;
      }
      if (!svar.ok) throw new Error('Kunne ikke opprette opplastingen');

      const { presigned_url, public_url } = await svar.json();

      const put = await fetch(presigned_url, {
        method: 'PUT',
        headers: { 'Content-Type': mime_type },
        body: blob,
      });
      if (!put.ok) throw new Error('Opplastingen til bøtta feilet');

      settInnMarkdown(`\n\n![](${public_url})`);
    } catch (feil) {
      setOpplastingsFeil(feil instanceof Error ? feil.message : 'Opplasting feilet');
    } finally {
      setOpplasting(false);
    }
  }

  function vedSlipp(e) {
    e.preventDefault();
    setDrarOver(false);
    const fil = e.dataTransfer?.files?.[0];
    if (fil) lastOpp(fil);
  }

  return html`
    <form
      class=${drarOver ? 'editor editor-drar' : 'editor'}
      method="post"
      action=${handling}
      onDragOver=${(e) => {
        if (e.dataTransfer?.types?.includes('Files')) {
          e.preventDefault();
          setDrarOver(true);
        }
      }}
      onDragLeave=${() => setDrarOver(false)}
      onDrop=${vedSlipp}
    >
      <label for="tittel">Tittel</label>
      <input
        id="tittel"
        name="tittel"
        value=${tittel}
        onInput=${(e) => setTittel(e.target.value)}
        placeholder="F.eks. Cusco og høydesyke"
      />

      <div class="editor-rader">
        <div>
          <label for="innhold">Innhold (Markdown)</label>
          <textarea
            id="innhold"
            name="innhold"
            rows="18"
            value=${innhold}
            onInput=${(e) => setInnhold(e.target.value)}
            ref=${tekstfelt}
          ></textarea>
        </div>

        <div>
          <span class="etikett">Forhåndsvisning</span>
          <article class="forhandsvisning">
            ${tittel && html`<h2>${tittel}</h2>`}
            <div class="brodtekst" dangerouslySetInnerHTML=${{ __html: forhåndsvist }}></div>
          </article>
        </div>
      </div>

      <input
        type="file"
        accept="image/jpeg,image/png,image/webp"
        hidden=${true}
        ref=${filInput}
        onChange=${(e) => {
          const fil = e.target.files?.[0];
          if (fil) lastOpp(fil);
          e.target.value = '';
        }}
      />

      <div class="knapperad">
        <button type="submit" name="handling" value="lagre">
          ${publisert ? 'Lagre' : 'Lagre kladd'}
        </button>
        ${publisert
          ? html`<button type="submit" name="handling" value="avpubliser" class="sekundaer">Avpubliser</button>`
          : html`<button type="submit" name="handling" value="publiser" class="sekundaer">Publiser</button>`}
        <button type="button" class="sekundaer" onClick=${() => filInput.current?.click()} disabled=${opplasting}>
          ${opplasting ? 'Laster opp …' : 'Last opp bilde'}
        </button>
        ${harUlagretKladd
          && html`<button type="button" class="sekundaer" onClick=${forkast}>Forkast endringer</button>`}
        <a href=${avbryt} class="sekundaer">Avbryt</a>
      </div>

      ${opplastingsFeil && html`<p class="feil">${opplastingsFeil}</p>`}

      <p class="hint">
        Kladden lagres også lokalt i nettleseren mens du skriver, så den overlever dårlig nett.
        Dra-og-slipp bilder i feltet, eller bruk «Last opp bilde».
      </p>
    </form>
  `;
}

const rot = document.getElementById('editor');
const enkeltSkjema = document.getElementById('editor-enkel');

// Etter en vellykket lagring redirecter serveren med ?lagret=1. Da er det vi
// nettopp skrev «committet» – slett den lokale kladden *før* Editor leser den,
// ellers overskygger den serverens ferske innhold. «ny»-nøkkelen ryddes også,
// siden en ny post flyttes fra /ny til /rediger/{slug}.
if (new URLSearchParams(location.search).has('lagret')) {
  localStorage.removeItem(kladdNøkkel(rot.dataset.slug));
  localStorage.removeItem(kladdNøkkel(''));
}

// Byttet skjer her, etter at alle importene er løst. Feiler en av dem, kastes
// det før denne linja og det enkle skjemaet blir stående – som er hele poenget
// med progressive enhancement.
if (enkeltSkjema) enkeltSkjema.remove();
rot.hidden = false;

render(
  html`<${Editor}
    slug=${rot.dataset.slug}
    handling=${rot.dataset.handling}
    startTittel=${rot.dataset.tittel}
    startInnhold=${rot.dataset.innhold}
    avbryt=${rot.dataset.avbryt}
    publisert=${rot.dataset.publisert === 'ja'}
  />`,
  rot,
);
