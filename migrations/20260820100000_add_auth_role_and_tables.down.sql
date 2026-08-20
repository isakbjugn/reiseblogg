-- Reverserer steg 3-auth: dropp sesjoner og engangskoder før rolle-kolonnen,
-- siden tabellene refererer author(id).
DROP TABLE IF EXISTS session;
DROP TABLE IF EXISTS magic_tokens;

ALTER TABLE author DROP COLUMN IF EXISTS role;
