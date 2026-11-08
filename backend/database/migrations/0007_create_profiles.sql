CREATE TABLE profiles (
    id INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    name TEXT NOT NULL,
    name_key TEXT NOT NULL,
    data_json TEXT NOT NULL CHECK (json_valid(data_json)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER
) STRICT;

CREATE UNIQUE INDEX profiles_user_name_key_unique
    ON profiles (user_id, name_key);
