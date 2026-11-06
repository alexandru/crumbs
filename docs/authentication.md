# Authentication

Crumbs runs a self-hosted OAuth authorization server for its REST and MCP APIs. Accounts are local. Create them through the CLI or the Rust database API; there is no self-service account-registration route. The browser sign-in page appears only while a user authorizes an OAuth client.

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

Usernames are ASCII and normalize to lowercase. They contain 1–64 characters: the first is an ASCII letter or digit, and later characters may also be `.`, `_`, or `-`. Passwords at creation must contain at least 6 Unicode scalar values and no more than 72 UTF-8 bytes. Crumbs hashes passwords with bcrypt cost 12 and does not truncate them.

The storage-independent `crumbs_database::identity::Users` API creates, looks up, lists, changes, deactivates, and deletes users through `create_user`, `user_by_username`, `user_by_id`, `list_users`, `set_user_password`, `set_user_active`, and `delete_user`. `PasswordHash::hash` creates the stored hash. Password changes and deactivation revoke the user's sessions atomically; deletion cascades to the user's sessions and authorizations. `crumbs_database::identity::OAuthStore::revoke_user_sessions` provides explicit session revocation.

## OAuth client flow

Clients register at `POST /oauth/register`. Dynamic client registration returns a client ID; confidential clients also receive a client secret, which must be stored securely by the client. Crumbs supports public clients (`none`) and confidential clients using `client_secret_basic` or `client_secret_post`. A client may request refresh tokens during registration.

Authorization uses the authorization-code flow with PKCE `S256`. Crumbs validates registered redirect URIs and accepts a changed port only for loopback IP redirect URIs. Client names and other metadata are untrusted. Crumbs validates the requested client, redirect URI, resource, and scope, then shows the user the client name, redirect origin, and requested access. To approve a request, the user enters a username and password and confirms consent; the user can cancel without signing in. There is no remembered browser login and no automatic approval.

Crumbs intentionally omits fetching Client ID Metadata Documents (CIMD). The current MCP authorization specification recommends CIMD support with `SHOULD` and permits Dynamic Client Registration (DCR) as a compatibility path with `MAY`. Crumbs avoids fetching client-controlled URLs to reduce its server-side request forgery (SSRF) surface, so clients must use DCR. See the [MCP authorization specification](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization) for the protocol requirements and compatibility guidance.

The public homepage (`GET /`) is a simple welcome page. Food search and MCP APIs remain bearer-protected as described below. The following discovery and authorization routes are also public so clients can complete the OAuth handshake. They do not require an existing Crumbs bearer token; OAuth client authentication still applies to token and revocation requests when the client is confidential.

| Route | Purpose |
| --- | --- |
| `GET /.well-known/oauth-authorization-server` | Authorization-server metadata |
| `GET /.well-known/oauth-protected-resource` | REST resource metadata |
| `GET /.well-known/oauth-protected-resource/mcp` | MCP resource metadata |
| `POST /oauth/register` | Dynamic client registration |
| `GET /oauth/authorize` | Begin authorization and request consent |
| `POST /oauth/login` | Submit credentials and an approval or denial |
| `POST /oauth/token` | Redeem a code or rotate a refresh token |
| `POST /oauth/revoke` | Revoke a client token |
| `GET /assets/login.css` | Embedded sign-in stylesheet |

All REST and MCP API routes require an `Authorization: Bearer` access token. REST tokens have audience `<issuer>/`; MCP tokens have audience `<issuer>/mcp`. A token is accepted only for the resource audience selected during authorization. Crumbs supports two scopes:

| Scope | Permission |
| --- | --- |
| `crumbs:read` | Search and read the shared food library, including nutrition data. |
| `crumbs:write` | Create, replace, and delete custom foods. |

Clients may request either scope or both as a space-separated list. Crumbs normalizes the order and duplicate values. If a client omits scope during registration or authorization, Crumbs uses `crumbs:read`. The authorization page shows the permissions requested by the client. A REST request requires `crumbs:read` for `GET` and `HEAD`, and `crumbs:write` for `POST`, `PUT`, `PATCH`, and `DELETE`. MCP calls use the requested tool's permission. An operation without the required permission returns HTTP `403` with a Bearer `WWW-Authenticate` challenge that identifies the required scope and protected resource metadata. MCP clients can request the additional permission through OAuth and retry the operation.

The token endpoint does not support reducing or increasing the approved scope. When a token request explicitly includes scope, it must match the approved scope. When omitted, the issued token keeps the approved scope. Refresh tokens retain the original approved scope.

All active users can read the same shared food catalogue; Crumbs does not currently separate catalogue access by user or role.

## Token lifetimes and revocation

| Credential or state | Lifetime |
| --- | --- |
| Pending login request and CSRF cookie | 10 minutes |
| Authorization code | 60 seconds |
| Access token | 10 minutes |
| Session and refresh-token family | 30 days absolute |

The browser CSRF cookie is request-bound, `HttpOnly`, and `SameSite=Lax`. HTTPS issuers use a `Secure`, `__Host-` cookie. Each authorization grant begins with a new sign-in request. Access tokens expire after 10 minutes. Refresh-token use rotates the token; replay of an old refresh token revokes the session family. Redeeming an authorization code is also one-time. Replaying a redeemed code revokes its session family. `POST /oauth/revoke` follows [RFC 7009](https://www.rfc-editor.org/rfc/rfc7009).

Password changes, deactivation, and deletion invalidate existing sessions and unredeemed authorizations. The server prunes expired OAuth data every minute. It removes unused clients after 30 days when they have no authorization or session records. The database accepts up to 4,096 registered clients and 4,096 pending authorization requests.

## Rate limits

Rate limits are held in memory by each server process and are bounded to 4,096 tracked keys. Limits are per minute unless noted:

| Operation | Limit |
| --- | --- |
| OAuth requests, global | 600 |
| Client registration, per socket peer | 10 |
| Authorization, per socket peer | 60 |
| Login, per socket peer | 30 |
| Token and revocation, per socket peer | 120 |
| Login attempts, per normalized username | 10 per 5 minutes |
| Concurrent bcrypt checks | 4 |

Deployments with multiple server processes need a shared rate limiter to enforce limits across those processes.

## Security invariants

- Passwords are stored as bcrypt hashes. The database stores SHA-256 digests of random codes, access tokens, refresh tokens, CSRF tokens, and confidential-client secrets, not their plaintext values.
- Authorization-code consumption, refresh rotation, replay revocation, and user-wide session revocation are transactional. These operations must remain atomic if persistence changes.
- Access tokens are opaque and database-backed. API authentication checks the token digest, resource audience, expiry, active user, and grant scope.
- The server does not trust proxy-supplied host or client-IP values. The configured issuer controls redirects, audience checks, and secure cookie behavior.
- Client metadata is untrusted input. The sign-in page renders it as escaped text and uses one embedded stylesheet. It loads no external fonts, images, scripts, or stylesheets.
- Unknown usernames use a dummy bcrypt hash and receive the same generic message as an incorrect password. Passwords, client secrets, and token values must never be logged or persisted in plaintext.

## References

- [MCP authorization specification](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)
- [OAuth Dynamic Client Registration, RFC 7591](https://www.rfc-editor.org/rfc/rfc7591)
- [Proof Key for Code Exchange, RFC 7636](https://www.rfc-editor.org/rfc/rfc7636)
- [OAuth 2.0 Token Revocation, RFC 7009](https://www.rfc-editor.org/rfc/rfc7009)
