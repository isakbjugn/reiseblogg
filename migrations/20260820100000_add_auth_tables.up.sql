-- Auth for steg 3: engangskoder (magic_tokens) og sesjoner (session).
-- Tabellene er hentet fra fagord-rust-api, men tabellen heter `author` her,
-- ikke `contributor`, så fremmednøklene peker på `author(id)`.

-- Engangskoder for kode-innlogging. En kode beviser at brukeren har tilgang til
-- e-postkontoen sin; ved verifisering byttes den inn i en sesjon. Databasen lagrer
-- kun en SHA-256-hash av koden, aldri klartekst, så en lekkasje ikke gir innlogging.
CREATE TABLE IF NOT EXISTS magic_tokens (
    id         SERIAL PRIMARY KEY,
    author_id  INT NOT NULL REFERENCES author(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL,                          -- SHA-256 av koden, aldri klartekst
    attempts   INT NOT NULL DEFAULT 0,                 -- feilede forsøk; låser etter en grense
    expires_at TIMESTAMPTZ NOT NULL,                   -- now() + 15 min
    used_at    TIMESTAMPTZ,                            -- settes ved forbruk (engangsbruk)

    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Oppslag av gjeldende kode for én forfatter (ved verifisering og når tidligere
-- ubrukte koder ugyldiggjøres før en ny utstedes).
CREATE INDEX magic_tokens_author_idx ON magic_tokens (author_id);

-- Sesjoner: utstedes når en forfatter har bevist seg med en gyldig engangskode.
-- Sesjonstokenet sendes til klienten i en cookie; databasen lagrer kun en hash,
-- så en lekkasje ikke gir kaprede sesjoner. Serveren er sannhetskilden for hvem
-- som er pålogget, og kan invalidere en sesjon ved utlogging.
CREATE TABLE IF NOT EXISTS session (
    id         SERIAL PRIMARY KEY,
    author_id  INT NOT NULL REFERENCES author(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL,                          -- SHA-256 av sesjonstokenet, aldri klartekst
    expires_at TIMESTAMPTZ NOT NULL,                   -- now() + 30 dager

    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Hvert autentisert kall slår opp sesjonen på tokenets hash. Unik fordi et
-- tilfeldig høy-entropi-token aldri skal kollidere, og indeksen gjør oppslaget raskt.
CREATE UNIQUE INDEX session_token_hash_idx ON session (token_hash);

-- Sletting av en forfatter rydder sesjonene deres; indeksen gjør det og evt.
-- «logg ut overalt» raskt.
CREATE INDEX session_author_idx ON session (author_id);
