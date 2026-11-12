PRAGMA writable_schema = ON;

UPDATE sqlite_schema
SET sql = replace(sql, ' CHECK (entry_type IN (''food'', ''biometric''))', '')
WHERE type = 'table' AND name = 'profile_logs';

PRAGMA writable_schema = RESET;

-- This index also advances schema_version so other connections reload the table definition.
CREATE INDEX profile_logs_profile_entry_type_id
    ON profile_logs (profile_id, entry_type, id);
