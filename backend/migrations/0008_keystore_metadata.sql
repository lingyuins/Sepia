CREATE TABLE keystore_metadata (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    nonce TEXT NOT NULL,
    ciphertext TEXT NOT NULL
);
