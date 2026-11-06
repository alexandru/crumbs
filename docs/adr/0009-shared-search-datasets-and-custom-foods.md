# Use shared search datasets and manage custom foods in Crumbs

Datasets classify foods as `common`, `branded`, or `custom` independently of their source. Search returns the best matches separately for each dataset, with independent limits, so clients and agents can choose among datasets without a large branded catalog excluding useful common or custom matches. This reuses the dataset concept instead of adding another classification. Source and source food ID retain provenance and identity; the original provider-specific dataset subdivision is not retained.

Custom foods share the food table and nutrition model with imported foods, while Crumbs owns their identities and lifecycle through a dedicated Rust management API, REST endpoints, and MCP tools. Management writes are restricted to custom foods and require explicit write permission. Optional notes can describe ingredients and are returned with food data without participating in full-text search.
