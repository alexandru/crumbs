# Profile log API

Profile logs record food consumption, biometric measurements, journal entries, and exercise sessions. Each log belongs to one profile. REST and MCP enforce the same profile ownership and read/write permissions.

## Journal entries

A journal entry contains free text about feelings, experiences, or observations. A profile can have multiple journal entries on the same day.

Create an entry with `POST /profiles/{profile_id}/logs`:

```json
{
  "day": "2026-11-12",
  "entry": {
    "type": "journal",
    "text": "Felt more rested today.\nThe afternoon walk helped."
  }
}
```

The `day` is required. The optional `time` uses the profile's local time. When time is omitted or null, it resolves to the current clock time in the profile's timezone, including for a past day.

The `text` field is required. Empty text and text that contains only whitespace are invalid. Valid text is stored exactly as supplied, including Unicode, line breaks, and surrounding whitespace.

The response contains the resolved day and time, log ID, profile ID, and a `data` object:

```json
{
  "type": "journal",
  "text": "Felt more rested today.\nThe afternoon walk helped."
}
```

Use `GET /profiles/{profile_id}/logs/{log_id}` to read one entry. Use `PATCH` on the same path to replace the journal text:

```json
{
  "entry": {
    "type": "journal",
    "text": "An updated reflection."
  }
}
```

A patch can change `day` or `time` without changing the text. An entry patch must include replacement text. A log cannot change its entry type. Use `DELETE` on the same path to delete the entry.

The MCP tools are `create_profile_log`, `get_profile_log`, `update_profile_log`, `delete_profile_log`, and `list_profile_logs`. Create and update use the same journal entry shape inside their existing request objects.

## Exercise entries

An exercise entry records an activity name, intensity, duration, calories burned, and optional notes. The activity name is free text, such as `walking` or `weights training`.

Create an entry with `POST /profiles/{profile_id}/logs`:

```json
{
  "day": "2026-11-13",
  "entry": {
    "type": "exercise",
    "exercise_type": "walking",
    "intensity": "moderate",
    "duration": "30 min",
    "calories_burned": "150 kcal",
    "notes": "Walk through the park."
  }
}
```

The activity name must not be blank. Intensity is `easy`, `moderate`, `hard`, or `very_hard`. Duration is a positive decimal with the `min` unit. Calories burned are a nonnegative decimal with the `kcal` unit. Both quantities use exact decimal strings. Calories are supplied with the entry. Notes can be omitted, null, or any string; text is preserved as supplied.

The response retains these fields in `data`. The existing occurrence day and time rules apply. REST and MCP use the same exercise fields.

Use `PATCH /profiles/{profile_id}/logs/{log_id}` to change any subset of exercise fields:

```json
{
  "entry": {
    "type": "exercise",
    "duration": "45 min",
    "notes": null
  }
}
```

Patches follow the shared field rules: omitted fields retain their values, null clears optional fields, and required fields cannot be cleared. Notes are optional; the activity name, intensity, duration, and calories burned are required. A log cannot change its entry type. Exercise entries support the same get, delete, ownership, and pagination rules as other logs. Exercise writes do not change the profile's cached current weight.

## Filtering

List logs with `GET /profiles/{profile_id}/logs`. The optional `entry_type` selects one type:

| Value | Entries |
| --- | --- |
| `food` | Food consumption |
| `biometric` | Biometric measurements |
| `journal` | Journal entries |
| `exercise` | Exercise sessions |

Omit `entry_type` to return all types. For example, `GET /profiles/42/logs?entry_type=journal&from=2026-11-01&to=2026-11-12` returns journal entries in the inclusive day range.

MCP uses `list_profile_logs` with `profile_id` and a `filter` object:

```json
{
  "profile_id": 42,
  "filter": {
    "entry_type": "journal",
    "from": "2026-11-01",
    "to": "2026-11-12"
  }
}
```

The existing `subtype=weight` filter requires `entry_type=biometric`. It cannot be combined with `journal` or `exercise`.

Lists contain at most 100 entries in descending ID order. This is creation order, independent of the occurrence day and time. The response has `items` and `last_id`. To get the next page, keep the same filters and pass the returned `last_id` as an exclusive cursor. An empty page has a null cursor.

## Existing databases

The forward migration removes the database's fixed list of entry types through a schema-only update. Existing log content, IDs, timestamps, indexes, and the ID counter are preserved. An index supports filtering and pagination by entry type. Supported entry types are validated by the Rust and API models. Journal create, edit, and delete operations do not change the profile's cached current weight.
