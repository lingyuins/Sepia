-- Sepia gateway initial schema.
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE admins (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  username TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE sessions (
  token_hash TEXT PRIMARY KEY,
  username TEXT NOT NULL,
  expires_at TEXT NOT NULL
);
CREATE TABLE providers (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE,
  protocol TEXT NOT NULL,
  endpoint TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE provider_keys (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  provider_id INTEGER NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
  nonce TEXT NOT NULL,
  ciphertext TEXT NOT NULL,
  disabled INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE provider_models (
  provider_id INTEGER NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
  model TEXT NOT NULL,
  input_price_micros INTEGER NOT NULL DEFAULT 0,
  output_price_micros INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (provider_id, model)
);
CREATE TABLE routes (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  slug TEXT NOT NULL UNIQUE,
  patterns TEXT NOT NULL,
  is_default INTEGER NOT NULL DEFAULT 0,
  enabled INTEGER NOT NULL DEFAULT 1,
  position INTEGER NOT NULL
);
CREATE TABLE route_candidates (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  route_id INTEGER NOT NULL REFERENCES routes(id) ON DELETE CASCADE,
  provider_id INTEGER NOT NULL REFERENCES providers(id),
  model TEXT NOT NULL,
  position INTEGER NOT NULL
);
CREATE TABLE client_keys (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  token_hash TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL,
  models TEXT NOT NULL,
  token_limit INTEGER NOT NULL,
  max_amount_micros INTEGER NOT NULL,
  rpm INTEGER NOT NULL,
  tpm INTEGER NOT NULL,
  expires_at TEXT,
  enabled INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE requests (
  id TEXT PRIMARY KEY,
  client_key_id INTEGER REFERENCES client_keys(id),
  protocol TEXT NOT NULL,
  model TEXT NOT NULL,
  status TEXT NOT NULL,
  input_tokens INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  cost_micros INTEGER NOT NULL DEFAULT 0,
  latency_ms INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);
CREATE TABLE attempts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  request_id TEXT NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
  provider_id INTEGER REFERENCES providers(id),
  provider_key_id INTEGER,
  model TEXT NOT NULL,
  status TEXT NOT NULL,
  error TEXT,
  input_tokens INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  latency_ms INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);
CREATE TABLE quota_usage (
  client_key_id INTEGER NOT NULL REFERENCES client_keys(id) ON DELETE CASCADE,
  period TEXT NOT NULL,
  requests INTEGER NOT NULL DEFAULT 0,
  tokens INTEGER NOT NULL DEFAULT 0,
  amount_micros INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (client_key_id, period)
);
CREATE TABLE quota_reservations (
  id TEXT PRIMARY KEY,
  client_key_id INTEGER NOT NULL REFERENCES client_keys(id),
  model TEXT NOT NULL,
  token_estimate INTEGER NOT NULL,
  amount_micros INTEGER NOT NULL,
  created_at TEXT NOT NULL
);
