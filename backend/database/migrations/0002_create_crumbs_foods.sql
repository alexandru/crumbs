CREATE TABLE crumbs_foods (
    id INTEGER PRIMARY KEY,
    source TEXT NOT NULL REFERENCES crumbs_sources(id),
    source_food_id TEXT NOT NULL,
    food_type TEXT NOT NULL,
    description TEXT NOT NULL CHECK (length(trim(description)) > 0),
    category TEXT,
    tags TEXT NOT NULL DEFAULT '',
    source_data TEXT NOT NULL CHECK (json_valid(source_data)),
    UNIQUE (source, source_food_id)
) STRICT;

INSERT INTO crumbs_sources (id) VALUES ('usda')
ON CONFLICT (id) DO NOTHING;
