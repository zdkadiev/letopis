CREATE TABLE meta
(
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    config_identity TEXT NOT NULL
);
CREATE TABLE vector_keys
(
    row_id      INTEGER PRIMARY KEY,
    search_hash TEXT NOT NULL UNIQUE
);
CREATE VIRTUAL TABLE vector_index USING vec1(vector);
CREATE TABLE query_error
(
    id    INTEGER PRIMARY KEY CHECK (id = 1),
    at    INTEGER NOT NULL,
    error TEXT    NOT NULL
);
CREATE TABLE embed_failures
(
    search_hash TEXT PRIMARY KEY,
    attempts    INTEGER NOT NULL CHECK (attempts >= 1),
    error       TEXT
);
INSERT INTO vector_index(cmd, arg)
VALUES ('rebuild', '{index:"none",distance:"cos"}');
