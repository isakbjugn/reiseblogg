-- Add up migration script here
CREATE TABLE IF NOT EXISTS author (
    id  SERIAL PRIMARY KEY,
    email TEXT NOT NULL,
    name TEXT NOT NULL
);

CREATE UNIQUE INDEX author_email_lower_idx ON author (lower(email));
