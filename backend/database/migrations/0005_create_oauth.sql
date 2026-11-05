CREATE TABLE oauth_clients (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) > 0),
    name TEXT NOT NULL CHECK (length(name) > 0),
    redirect_uris TEXT NOT NULL CHECK (json_valid(redirect_uris)),
    authentication TEXT NOT NULL CHECK (authentication IN ('public', 'secret_basic', 'secret_post')),
    secret_hash BLOB CHECK (secret_hash IS NULL OR (typeof(secret_hash) = 'blob' AND length(secret_hash) = 32)),
    allow_refresh INTEGER NOT NULL CHECK (allow_refresh IN (0, 1)),
    created_at INTEGER NOT NULL,
    CHECK (
        (authentication = 'public' AND secret_hash IS NULL)
        OR (authentication IN ('secret_basic', 'secret_post') AND secret_hash IS NOT NULL)
    )
) STRICT;

CREATE TABLE users_sessions (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    client_id TEXT NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    resource TEXT NOT NULL,
    scope TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
) STRICT;

CREATE INDEX users_sessions_by_user ON users_sessions (user_id, id);
CREATE INDEX users_sessions_by_expiry ON users_sessions (expires_at, id);
CREATE INDEX users_sessions_by_client_resource ON users_sessions (client_id, resource, id);

CREATE TABLE oauth_authorizations (
    request_hash BLOB NOT NULL PRIMARY KEY CHECK (typeof(request_hash) = 'blob' AND length(request_hash) = 32),
    csrf_hash BLOB NOT NULL CHECK (typeof(csrf_hash) = 'blob' AND length(csrf_hash) = 32),
    client_id TEXT NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    redirect_uri TEXT NOT NULL,
    resource TEXT NOT NULL,
    scope TEXT NOT NULL,
    state TEXT,
    code_challenge TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    user_id INTEGER REFERENCES users(id) ON DELETE CASCADE,
    code_hash BLOB UNIQUE CHECK (code_hash IS NULL OR (typeof(code_hash) = 'blob' AND length(code_hash) = 32)),
    session_id INTEGER REFERENCES users_sessions(id) ON DELETE CASCADE,
    CHECK ((user_id IS NULL AND code_hash IS NULL AND session_id IS NULL) OR (user_id IS NOT NULL AND code_hash IS NOT NULL))
) STRICT;

CREATE INDEX oauth_authorizations_by_user ON oauth_authorizations (user_id, request_hash);
CREATE INDEX oauth_authorizations_by_client_expiry ON oauth_authorizations (client_id, expires_at);
CREATE INDEX oauth_authorizations_by_expiry ON oauth_authorizations (expires_at, session_id);

CREATE TABLE oauth_tokens (
    hash BLOB NOT NULL PRIMARY KEY CHECK (typeof(hash) = 'blob' AND length(hash) = 32),
    session_id INTEGER NOT NULL REFERENCES users_sessions(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('access', 'refresh')),
    expires_at INTEGER NOT NULL,
    used INTEGER NOT NULL DEFAULT 0 CHECK (used IN (0, 1))
) STRICT;

CREATE INDEX oauth_tokens_by_session ON oauth_tokens (session_id, kind, used);
CREATE INDEX oauth_tokens_by_expiry ON oauth_tokens (kind, expires_at, used);
