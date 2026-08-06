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

const KLADD_NØKKEL = 'reiseblogg:kladd';

function Editor({ startTittel, startInnhold }) {
  // Kladd fra localStorage vinner over serverens startverdi: mistet nett eller
  // et uhell med refresh skal ikke spise det du har skrevet. Kritisk på reise.
  const kladd = useMemo(() => {
    try {
      return JSON.parse(localStorage.getItem(KLADD_NØKKEL) ?? 'null');
    } catch {
      return null;
    }
  }, []);

  const [tittel, setTittel] = useState(kladd?.tittel ?? startTittel);
  const [innhold, setInnhold] = useState(kladd?.innhold ?? startInnhold);

  useEffect(() => {
    localStorage.setItem(KLADD_NØKKEL, JSON.stringify({ tittel, innhold }));
  }, [tittel, innhold]);

  // `html: false` slår av rå HTML i Markdown. Viktig: forhåndsvisningen settes
  // inn med dangerouslySetInnerHTML, og når dette senere rendres av serveren må
  // pulldown-cmark kjøre med samme innstilling – ellers divergerer de to.
  const md = useMemo(() => new MarkdownIt({ html: false, linkify: true, breaks: false }), []);
  const forhåndsvist = useMemo(() => md.render(innhold), [md, innhold]);

  return html`
    <form class="editor" method="post" action="/ny">
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
            <div dangerouslySetInnerHTML=${{ __html: forhåndsvist }}></div>
          </article>
        </div>
      </div>

      <p class="hint">
        Kladden lagres lokalt i nettleseren. Publisering kommer når databasen er på plass.
      </p>
    </form>
  `;
}

const rot = document.getElementById('editor');

render(
  html`<${Editor}
    startTittel=${rot.dataset.tittel}
    startInnhold=${rot.dataset.innhold}
  />`,
  rot,
);
