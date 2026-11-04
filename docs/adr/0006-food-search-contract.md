# Expose SQLite FTS5 expressions for food search

Food search exposes the existing SQLite FTS5 query language over food descriptions, categories, and tags rather than introducing a separate query language or parser. Queries are bound SQL parameters; SQLite interprets their FTS5 syntax; malformed expressions reported by SQLite become HTTP 400 Bad Request responses and MCP tool errors, without an application-level FTS5 parser. This keeps search aligned with the existing index, at the cost of making FTS5 syntax part of the public API contract.

The database crate provides the search operation. The server exposes the same behavior through `GET /foods/search` with `query` and optional `limit` parameters and through an MCP `search_foods` tool over Streamable HTTP at `/mcp`, allowing applications and hosted agent harnesses to share one search contract.

Results include full nutrition data and use SQLite's default relevance ranking, with food ID breaking ties. Search has no pagination: an omitted limit defaults to 20; a supplied limit is clamped with `max(min(limit, 1000), 20)`. Empty queries are rejected. Access is public initially; authentication is deferred.
