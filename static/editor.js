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
import { useEffect, useMemo, useState } from 'preact/hooks';
import { html } from 'htm/preact';
import MarkdownIt from 'markdown-it';

// Én nøkkel per post, slik at en kladd på «Cusco» ikke overskriver en kladd på
// «Atacama». Ny post får nøkkelen «ny».
const kladdNøkkel = (slug) => `reiseblogg:kladd:${slug || 'ny'}`;

// `html: false` slår av rå HTML i Markdown. Viktig: forhåndsvisningen settes inn
// med dangerouslySetInnerHTML, og serveren rendrer det publiserte med
// pulldown-cmark på samme innstilling – ellers divergerer de to. Se CLAUDE.md.
const md = new MarkdownIt({ html: false, linkify: true, breaks: false });

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

  return html`
    <form class="editor" method="post" action=${handling}>
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

      <div class="knapperad">
        <button type="submit" name="handling" value="lagre">
          ${publisert ? 'Lagre' : 'Lagre kladd'}
        </button>
        ${publisert
          ? html`<button type="submit" name="handling" value="avpubliser" class="sekundaer">Avpubliser</button>`
          : html`<button type="submit" name="handling" value="publiser" class="sekundaer">Publiser</button>`}
        ${harUlagretKladd
          && html`<button type="button" class="sekundaer" onClick=${forkast}>Forkast endringer</button>`}
        <a href=${avbryt} class="sekundaer">Avbryt</a>
      </div>

      <p class="hint">
        Kladden lagres også lokalt i nettleseren mens du skriver, så den overlever dårlig nett.
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
