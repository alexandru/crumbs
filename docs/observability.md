# Logs

Crumbs writes timestamped logs to stderr. By default, Crumbs events use `info`
and dependency logs are disabled. Use `RUST_LOG=warn` to hide routine events.
Target filters are also supported:
`RUST_LOG=crumbs_import=info,warn` keeps import progress and hides other routine
events. With Docker Compose, pass the variable to the container with
`-e RUST_LOG=warn` on `docker compose run`.

## USDA imports

Imports log their start and result. During record processing, progress events
appear every five seconds, including during a slow database write. Each event
reports:

- `committed`: foods saved to the database.
- `sanitized` and `rejected`: foods with corrected values and foods skipped.
- `elapsed_secs` and `foods_per_sec`: elapsed time and average saved foods per second.
- `phase`: `waiting_for_record` or `writing`.

`import_completed` reports final counts. `import_failed` reports counts saved
before the failure. The final summary stays on stdout. Use `--verbose` for
individual food diagnostics.

## HTTP and MCP

HTTP logs report the method, route template, status, and time to response
headers. This time excludes streaming the response body. MCP tool logs report
the tool name, outcome, and elapsed time. Tool failures are logged even when
the HTTP status is `200`. Operations cancelled before a result log
`cancelled`.

Crumbs request logs exclude query strings, request bodies, credentials, tool
arguments, and tool results. Unknown routes and tool names use placeholders.
Dependency logs enabled through `RUST_LOG` can include client data.

For a Compose server, follow logs with `docker compose logs -f crumbs`.
Imports started with `docker compose run` print logs in the attached terminal.
