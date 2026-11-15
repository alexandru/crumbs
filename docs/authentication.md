# Authentication

Crumbs runs a self-hosted OAuth authorization server for its REST and MCP APIs. Accounts are local. Create them through the CLI; there is no self-service account-registration route. The browser sign-in page appears only while a user authorizes an OAuth client.

## Configure and run

Configure Crumbs with command-line options or environment variables. Set the issuer to the public origin clients use to reach Crumbs. A command-line option overrides its environment variable, and an environment variable overrides its default. Relative database paths use the process working directory.

| Setting | CLI option | Environment variable | Default |
| --- | --- | --- | --- |
| Database file | `--database-path PATH` | `CRUMBS_DATABASE_PATH` | Required |
| HTTP bind address | `--http-bind ADDRESS` | `CRUMBS_HTTP_BIND` | `127.0.0.1` |
| HTTP port | `--http-port PORT` | `CRUMBS_HTTP_PORT` | `8080` |
| OAuth issuer | `--oauth-issuer ORIGIN` | `CRUMBS_OAUTH_ISSUER` | Required |

Start the server with an option:

```text
crumbs server start --database-path ./crumbs.db --oauth-issuer https://crumbs.example.invalid
```

Or set the database path and issuer in the environment. The HTTP bind address and port then use their defaults:

```sh
export CRUMBS_DATABASE_PATH=./data/crumbs.db
export CRUMBS_OAUTH_ISSUER=https://crumbs.example.invalid
crumbs server start
```

The issuer must be an origin without credentials, path, query, or fragment. Use HTTPS for a public issuer. HTTP is allowed for an IP loopback origin during local development, such as `http://127.0.0.1:8080`. Run `crumbs server start --help` to see each option and its environment variable.

The server does not terminate TLS. Put a reverse proxy in front of it, set `issuer` to the externally visible HTTPS origin, and keep the HTTP listener private. Binding to `0.0.0.0` requires network controls that prevent public access to the plaintext listener. The server does not trust forwarded host or client-IP headers. Rate limits use the socket peer address, so requests passing through one proxy share its peer bucket.

The Docker image creates the writable `/app/data` directory and sets `CRUMBS_DATABASE_PATH=/app/data/crumbs.db`, `CRUMBS_HTTP_BIND=0.0.0.0`, and `CRUMBS_HTTP_PORT=8080`. The mounted `/app/data` volume in the Compose example in the [README](../README.md) preserves the database. Set `CRUMBS_OAUTH_ISSUER` to the public HTTPS origin in the container environment. Keep any published host port private behind a TLS reverse proxy.

## Create and manage users

Create a user with a hidden password prompt and confirmation. This command needs only the database path. Set `CRUMBS_DATABASE_PATH` or pass `--database-path`. The username can come from `CRUMBS_USERNAME` or `--username`.

```text
crumbs db create-user --username alice --database-path ./data/crumbs.db
CRUMBS_DATABASE_PATH=./crumbs.db crumbs db create-user --username alice
```

For USDA imports, `CRUMBS_IMPORT_FILE` can supply the JSON file path. `CRUMBS_VERBOSE` accepts `true` or `false` and defaults to `false`. Pass `--verbose` to enable diagnostic details, or `--verbose=false` to disable them even when `CRUMBS_VERBOSE=true`.

Usernames are ASCII and normalize to lowercase. They contain 1–64 characters: the first is an ASCII letter or digit, and later characters may also be `.`, `_`, or `-`. Passwords at creation must contain at least 6 Unicode scalar values and no more than 72 UTF-8 bytes.
