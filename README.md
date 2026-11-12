# 🍽️ Crumbs

**Your food diary, one conversation away.**

A self-hosted nutrition tracker for ChatGPT, Mistral, and Claude. Search USDA foods, add your own, and track meals, calories, nutrients, and weight through conversation.

## Get started

<details>
<summary><b>1. Start with Docker Compose</b></summary>

Save as `compose.yaml` and replace the domain with yours. Route public HTTPS traffic to `localhost:8080` through a [reverse proxy](docs/authentication.md#configure-and-run).

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

```sh
docker compose up -d
```

</details>

<details>
<summary><b>2. Import USDA foods</b></summary>

Download [USDA Foundation Foods (JSON)](https://fdc.nal.usda.gov/download-datasets/), unzip it, and save the JSON file as `~/Downloads/usda.json`.

```sh
docker compose run --rm --no-deps \
  --volume "$HOME/Downloads/usda.json:/imports/usda.json:ro" \
  crumbs db import-usda /imports/usda.json
```

</details>

<details>
<summary><b>3. Create your user</b></summary>

Replace `alice` with your username. Enter a password when prompted.

```sh
docker compose run --rm --no-deps crumbs db create-user --username alice
```

</details>

<details>
<summary><b>4. Connect your assistant</b></summary>

- [**ChatGPT**](https://developers.openai.com/api/docs/guides/custom-mcp-server): **Plugins → + → Add custom MCP server**.
- [**Mistral**](https://docs.mistral.ai/vibe/work/connectors/mcp-connectors): **Connectors → + Add Connector → Custom MCP Connector**.

Enter `https://crumbs.example.com/mcp` (with your domain), use OAuth, and sign in with your Crumbs account.

</details>
