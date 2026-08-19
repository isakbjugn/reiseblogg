-- Add down migration script here
DROP TABLE IF EXISTS post;

-- set_updated_at() ble definert i opp-migrasjonen og nedlegges sammen med den.
-- Triggeren trenger ikke nedlegges separat – den forsvinner med tabellen.
DROP FUNCTION IF EXISTS set_updated_at;
