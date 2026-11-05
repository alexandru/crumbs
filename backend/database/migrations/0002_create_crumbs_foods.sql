CREATE TABLE crumbs_foods (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL REFERENCES crumbs_sources(id),
    source_food_id TEXT NOT NULL,
    dataset TEXT NOT NULL,
    description TEXT NOT NULL CHECK (length(trim(description)) > 0),
    category TEXT,
    parsed_data TEXT NOT NULL CHECK (json_valid(parsed_data)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER,
    UNIQUE (source, source_food_id)
) STRICT;

CREATE TABLE crumbs_tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    name_key TEXT NOT NULL UNIQUE CHECK (length(name_key) > 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER
) STRICT;

CREATE TABLE crumbs_food_tags (
    food_id INTEGER NOT NULL REFERENCES crumbs_foods(id) ON DELETE CASCADE ON UPDATE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES crumbs_tags(id) ON DELETE CASCADE ON UPDATE CASCADE,
    created_at INTEGER NOT NULL,
    updated_at INTEGER,
    PRIMARY KEY (food_id, tag_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX crumbs_food_tags_by_tag ON crumbs_food_tags (tag_id, food_id);
