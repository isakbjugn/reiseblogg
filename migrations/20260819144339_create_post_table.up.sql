-- Add up migration script here
-- set_updated_at() er hentet fra fagord-rust-api (egen migrasjon der). Den er
-- generisk – hver tabell får sin egen trigger, jf. trg_set_updated_at under.
-- Den finnes ikke i denne databasen ennå, så den defineres her: post er første
-- tabellen med updated_at.
CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = now();
RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Bloggposter. `published_at IS NULL` betyr kladd.
-- Slug genereres fra tittel ved oppretting og låses i applikasjonen etterpå –
-- publiserte lenker skal ikke brytes. Databasen garanterer bare unikhet.
CREATE TABLE IF NOT EXISTS post (
    id SERIAL PRIMARY KEY,
    slug TEXT UNIQUE NOT NULL,
    title TEXT NOT NULL,
    content TEXT NOT NULL,                                       -- Markdown
    published_at TIMESTAMPTZ,                                    -- NULL = kladd

    created_by INT NOT NULL REFERENCES author(id),               -- eier; grunnlag for autorisasjon
    updated_by INT REFERENCES author(id),                        -- sist endret av (null på ny post)

    created_at TIMESTAMPTZ DEFAULT now() NOT NULL,
    updated_at TIMESTAMPTZ DEFAULT now() NOT NULL
);

CREATE TRIGGER trg_set_updated_at
    BEFORE UPDATE ON post
    FOR EACH ROW
    EXECUTE FUNCTION set_updated_at();
