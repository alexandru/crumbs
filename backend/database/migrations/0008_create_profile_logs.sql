CREATE TABLE profile_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE RESTRICT,
    day TEXT NOT NULL CHECK (length(day) = 10),
    time TEXT NOT NULL CHECK (length(time) = 12),
    data_json TEXT NOT NULL CHECK (json_valid(data_json)),
    entry_type TEXT GENERATED ALWAYS AS (
        json_extract(data_json, '$.type')
    ) STORED NOT NULL CHECK (entry_type IN ('food', 'biometric')),
    subtype TEXT GENERATED ALWAYS AS (
        CASE
            WHEN json_extract(data_json, '$.type') = 'biometric'
            THEN json_extract(data_json, '$.measurement.type')
            ELSE NULL
        END
    ) STORED,
    source_id INTEGER GENERATED ALWAYS AS (
        CASE
            WHEN json_extract(data_json, '$.type') = 'food'
            THEN json_extract(data_json, '$.food_id')
            ELSE NULL
        END
    ) STORED,
    created_at INTEGER NOT NULL,
    updated_at INTEGER
) STRICT;

CREATE INDEX profile_logs_profile_id_id
    ON profile_logs (profile_id, id);

CREATE INDEX profile_logs_profile_day_id
    ON profile_logs (profile_id, day, id);

CREATE INDEX profile_logs_latest_weight
    ON profile_logs (
        profile_id,
        entry_type,
        subtype,
        day DESC,
        time DESC,
        created_at DESC,
        id DESC
    );
