# Private application-managed persistence timestamps

Application-managed database timestamps such as `created_at` and `updated_at` use signed 64-bit Unix milliseconds stored as SQLite STRICT-compatible `INTEGER` columns. Application code generates them rather than database clock defaults or timestamp triggers, avoiding database-specific datetime handling. These values belong to the SQLite integration, not public APIs or domain types; this policy does not change dates inside source payloads.

Insertion sets `created_at` and leaves `updated_at` NULL. Updates preserve `created_at` and set `updated_at` from the application clock. Conflicts that leave immutable sources, first-spelling tag catalog entries, or retained tag assignments unchanged are not updates. Removed assignments are deleted; assigning them again creates new rows.

The integration uses the system clock by default and permits private clock injection for deterministic SQLite write tests without sleeps. Clock values before the Unix epoch or outside the signed 64-bit millisecond range return errors before writes. The clock need not be monotonic, so `updated_at` can precede `created_at` after a clock adjustment.
