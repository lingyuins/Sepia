-- Provider channel modes: a single-channel provider keeps one address; a
-- unified-channel provider owns several addresses (each with its own keys)
-- that fail over between each other while serving the same models.
ALTER TABLE providers ADD COLUMN mode TEXT NOT NULL DEFAULT 'single';

CREATE TABLE provider_endpoints (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  provider_id INTEGER NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
  endpoint TEXT NOT NULL,
  position INTEGER NOT NULL
);

INSERT INTO provider_endpoints (provider_id, endpoint, position)
SELECT id, endpoint, 0 FROM providers;

ALTER TABLE provider_keys ADD COLUMN endpoint_id INTEGER REFERENCES provider_endpoints(id) ON DELETE CASCADE;

UPDATE provider_keys
SET endpoint_id = (
  SELECT id FROM provider_endpoints
  WHERE provider_endpoints.provider_id = provider_keys.provider_id
  ORDER BY position, id LIMIT 1
);