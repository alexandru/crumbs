# Crumbs

Crumbs is a self-hosted calorie tracker in early development. The first milestone is a food catalog that an MCP server can search and extend; the goal is to track daily caloric intake and burn.

## Run with Docker Compose

Requires Docker with Compose. Set `CRUMBS_OAUTH_ISSUER` to the public HTTPS origin; see [Authentication](docs/authentication.md). Save this as `compose.yaml`:

```yaml
services:
  crumbs:
    image: ghcr.io/alexandru/crumbs:latest
    environment:
      CRUMBS_OAUTH_ISSUER: https://crumbs.example.com
    ports:
      - "8080:8080"
    volumes:
      - crumbs-data:/app/data

volumes:
  crumbs-data:
```

From the same directory, start the server:

```sh
docker compose up -d
```

Download the [USDA Foundation Foods JSON dataset](https://fdc.nal.usda.gov/download-datasets/), extract the ZIP, and save the JSON file as `~/Downloads/usda.json`. Import it with:

```sh
docker compose run --rm --no-deps \
  --volume "$HOME/Downloads/usda.json:/imports/usda.json:ro" \
  crumbs db import-usda /imports/usda.json
```

Stop the service with `docker compose down`; the `crumbs-data` volume keeps the database.
