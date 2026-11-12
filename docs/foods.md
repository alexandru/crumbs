# Food API

REST and MCP expose the same food search and custom-food management operations. See [Authentication](authentication.md) for OAuth setup and resource audiences. Food data belongs to the shared Crumbs library. Custom foods are not private per-user records.

## Datasets and search

Each food has one source-independent dataset:

| Dataset | Foods |
| --- | --- |
| `common` | Generic reference foods, including USDA Foundation, SR Legacy, and Survey foods |
| `branded` | Imported branded products |
| `custom` | Foods created and managed directly in Crumbs |

The source identifies the provider, such as `usda`. Source and source food ID identify a food within a provider. Dataset does not participate in that identity.

REST search uses `GET /foods/search?query=chicken&limit=20`. The MCP tool is `search_foods` with arguments `{"query":"chicken","limit":20}`. Both return:

```json
{
  "common": [],
  "branded": [],
  "custom": []
}
```

Each array contains matching food records. All three keys are always present. The limit applies independently to each array: it defaults to 20 and is clamped to 20 through 1,000. Results within each dataset use best-effort relevance with local food ID breaking ties. Clients and agents choose which datasets and foods to use.

The query supports terms, quoted phrases, trailing `*` prefixes, `AND`, `OR`, `NOT`, parentheses, and selectors for `description`, `category`, `tags`, `source`, and `dataset`. Examples include `chicken dataset:common`, `source:usda`, and `tags:favorite`. Notes are excluded from all full-text searches. Search has no pagination. Invalid queries produce HTTP 400 or an MCP tool error.

## Custom-food management

| REST | MCP tool | Permission |
| --- | --- | --- |
| `GET /foods/custom?limit=20` | `list_custom_foods` | `crumbs:read` |
| `GET /foods/custom/{id}` | `get_custom_food` | `crumbs:read` |
| `POST /foods/custom` | `create_custom_food` | `crumbs:write` |
| `PUT /foods/custom/{id}` | `update_custom_food` | `crumbs:write` |
| `DELETE /foods/custom/{id}` | `delete_custom_food` | `crumbs:write` |

Create and replace accept this REST body:

```json
{
  "description": "Homemade hummus",
  "category": "Legumes",
  "notes": "Chickpeas, tahini, lemon juice, olive oil, garlic.",
  "parsed_data": {
    "nutrition": {
      "per": "100 g",
      "energy": "210 kcal",
      "protein": "7 g",
      "fat": {"total": "14 g"},
      "carbohydrates": {"excluding_fibre": "9 g"},
      "fibre": "6 g"
    },
    "serving_sizes": [
      {"name": "one serving", "quantity": "30 g"}
    ]
  }
}
```

Only `description` is required. It must contain a non-whitespace character and must not contain NUL characters. `category` and `notes` are optional text fields and may be null. `parsed_data` defaults to an unknown nutrition profile and no servings. Nutrient amounts use nonnegative decimal strings with units; serving quantities must be positive. Nutrition applies per `100 g` or `100 ml`. Missing nutrients remain unknown rather than becoming zero. Unknown top-level food input fields are rejected.

Crumbs assigns the local food ID and opaque source food ID. Both `source` and `dataset` are `custom`. Inputs cannot supply identities, a source, or a dataset. Each returned food is a flat record containing `id`, `source`, `source_food_id`, `dataset`, `description`, `category`, `notes`, and `parsed_data`. Notes are returned as text or null.

POST returns HTTP 201 with the full food record and a `Location` header. GET and PUT return HTTP 200 with the full food record. PUT replaces the editable data: omitted optional fields are cleared. Identity and tag assignments remain unchanged. DELETE returns HTTP 204 and removes the food's tag assignments and search entry. List returns `{"foods":[...]}` in ascending local food ID order, with the same default and bounded limit as search and no pagination.

MCP arguments are:

```json
{"food":{"description":"Homemade hummus","notes":"Chickpeas and tahini."}}
```

for `create_custom_food`, and:

```json
{"id":42,"food":{"description":"Homemade hummus","notes":"Updated ingredients."}}
```

for `update_custom_food`. `get_custom_food` and `delete_custom_food` take `{"id":42}`. `list_custom_foods` takes an optional `limit`. Create, get, and update return the full food record; list returns `{"foods":[...]}`; delete returns `{"id":42,"deleted":true}`.

Management reads and writes only address foods whose source and dataset are both `custom`. Missing IDs and imported-food IDs produce HTTP 404 or an MCP tool error. Invalid input produces HTTP 400 or an MCP tool error. Storage failures produce a generic HTTP 500 or MCP tool error without storage details. REST and MCP operations without the required permission receive HTTP 403 with a Bearer `WWW-Authenticate` challenge identifying `insufficient_scope`, the required scope, and the protected resource metadata URI. MCP clients can request the additional permission through OAuth and retry the operation.

## Nutrition

Nutrition uses the same fields for imported and custom foods. Water, caffeine, and choline are optional mass amounts at the same level as protein. Minerals and vitamins have separate groups:

```json
{
  "per": "100 g",
  "water": "75 g",
  "caffeine": "0 mg",
  "choline": "20 mg",
  "minerals": {
    "magnesium": "25 mg",
    "potassium": "200 mg"
  },
  "vitamins": {
    "vitamin_a": "50 µg",
    "vitamin_b9": "30 µg"
  }
}
```

The example shows the structure only. It does not describe a specific food. Each amount can be omitted or null when unknown. Omitted groups contain no known amounts. Amounts retain their mass units and qualifiers, such as `< 1 mg` or `~ 20 µg`. Existing fields, including sodium, keep their locations.

The mineral fields are `calcium`, `iron`, `magnesium`, `phosphorus`, `potassium`, `zinc`, `copper`, `manganese`, and `selenium`.

The vitamin fields have these meanings:

| Field | Measured form |
| --- | --- |
| `vitamin_a` | Retinol activity equivalents |
| `vitamin_c` | Total ascorbic acid |
| `vitamin_d` | Total vitamin D2 and D3 |
| `vitamin_e` | Alpha-tocopherol |
| `vitamin_k1` | Phylloquinone |
| `vitamin_b1` | Thiamin |
| `vitamin_b2` | Riboflavin |
| `vitamin_b3` | Niacin |
| `vitamin_b5` | Pantothenic acid |
| `vitamin_b6` | Vitamin B6 |
| `vitamin_b9` | Total folate |
| `vitamin_b12` | Vitamin B12 |

Vitamin amounts use mass units. Values in international units, dietary folate equivalents, or niacin equivalents must not be entered as mass amounts without a valid conversion to the specified form. Choline means total choline.

USDA imports map the specific nutrient forms from the [USDA nutrient catalog](https://fdc.nal.usda.gov/download-datasets/). To add available amounts to foods imported before these fields were supported, run the existing `db import-usda` command again with the USDA JSON file. Reimport preserves food IDs and updates food nutrition. It does not change stored food-log snapshots. New food-log entries include the available nutrients, scaled to the consumed quantity.

Custom-food `notes` can contain ingredients and recipe instructions as free text. Notes do not define structured ingredients or calculate nutrition.

## Rust API

The `crumbs_database::foods` module exposes `Dataset`, `Food`, `SearchResults`, and `CustomFoodInput`. `FoodSearch::search_foods` returns the three dataset arrays. `CustomFoodStore` exposes `create_custom_food`, `custom_food`, `list_custom_foods`, `update_custom_food`, and `delete_custom_food` with the same management boundaries and replacement semantics as HTTP. These traits keep storage-specific errors behind associated types.

## Existing databases

The forward migration adds nullable notes and converts `usda_foundation`, `usda_sr_legacy`, and `usda_survey_fndds` to `common`, and `usda_branded` to `branded`. Food identities, nutrition, and tag assignments are preserved. The original USDA subdivision is no longer stored, and clients must use the new dataset selectors and grouped response.

The migration fails atomically if any other unrecognized dataset remains. For a database populated through a custom Rust importer, inspect its distinct dataset values and explicitly map those foods to `common`, `branded`, or `custom` before upgrading. No classification is guessed for unknown identifiers.
