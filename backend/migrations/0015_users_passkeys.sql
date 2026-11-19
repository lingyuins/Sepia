-- Console users with roles, passkey credentials and one-time WebAuthn ceremonies.
CREATE TABLE users (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  username TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('admin', 'editor', 'viewer')),
  webauthn_user_id TEXT NOT NULL UNIQUE,
  created_at TEXT NOT NULL
);
INSERT INTO users (username, password_hash, role, webauthn_user_id, created_at)
SELECT username, password_hash, 'admin', lower(hex(randomblob(16))), created_at FROM admins;
CREATE TABLE webauthn_credentials (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  credential TEXT NOT NULL,
  created_at TEXT NOT NULL,
  last_used_at TEXT
);
CREATE TABLE webauthn_challenges (
  token_hash TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('register', 'login')),
  user_id INTEGER REFERENCES users(id) ON DELETE CASCADE,
  state TEXT NOT NULL,
  expires_at TEXT NOT NULL
);