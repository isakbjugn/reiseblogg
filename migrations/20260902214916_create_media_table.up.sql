-- Add up migration script here
CREATE TABLE IF NOT EXISTS media (
    id          SERIAL PRIMARY KEY,
    key         TEXT UNIQUE NOT NULL,
    mime_type   TEXT NOT NULL,
    uploaded_by INT NOT NULL REFERENCES author(id),
    created_at  TIMESTAMPTZ DEFAULT now() NOT NULL
);
