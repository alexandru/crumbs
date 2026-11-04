# Shared imported food data

Imported food details will be serialized as JSON in `parsed_data` using a shared model owned by Crumbs. Source adapters will translate external records into this model so consumers can access nutrition without parsing provider-specific schemas. Provider-specific raw documents are not the application food model: storing them unparsed made imported foods unusable for ordinary nutrition queries and coupled consumers to source schemas.

Food identity remains the combination of Source and Source food ID. USDA is the implementation scope; Open Food Facts informs compatibility.

Each food has at most one nutrition profile with a basis of either 100 grams or 100 millilitres; a food may have no nutrition data. Mass-based and volume-based profiles belong to separate foods. Published standardized source values are used when available; other explicitly measured source quantities require decimal normalization to the selected basis. Missing nutrient amounts use `Option` in code and are never defaulted to zero.

The initial profile covers energy; total, saturated, monounsaturated, polyunsaturated, and trans fat; carbohydrates with explicit meaning regarding fibre; total sugars, added sugars, and lactose; starch; fibre; protein; sodium; salt; and cholesterol. Source-supplied carbohydrate definitions remain distinct; fibre subtraction is not automatically stored. Added sugars and lactose may overlap and are not summed as disjoint components.

Foods may have multiple serving definitions, each with a known measured mass or volume. Serving definitions are stored separately from the nutrition profile; nutrient values for servings are calculated when needed rather than stored as redundant copies.

Nutrition quantities use fixed-point decimal values and arithmetic throughout parsing, conversion, and JSON persistence, preserving decimal values without intermediate binary floating point.

Reference and serving quantities must be positive; reported nutrient amounts must be nonnegative. A negative reported amount fails the import record rather than being clamped or treated as missing. This also applies to USDA's calculated negative carbohydrate-by-difference values, so a release containing them stops at the first such record while retaining earlier committed foods.

Serialized quantities use unit-bearing strings, such as `"15 mg"` or `"0.55 µg"`. Deserialization parses these strings directly into concrete measurement types backed by fixed-point decimals; unsupported or incompatible units are rejected at the boundary. Nutrient amounts retain their source measurement units where supported.

Reported nutrient amounts preserve source-supplied comparisons and approximations rather than treating them as unqualified numbers, using strings such as `"< 0.5 g"` and `"~ 15 mg"`. Parsing and serialization preserve selected source values without silent rounding. Any normalization that requires rounding needs an explicit precision and rounding policy.

Conversions are applied only when needed to standardize source data and when the known inputs support them. Store the supplied information without adding redundant derived values; for example, salt does not require a stored sodium conversion. Normalization to the profile basis is permitted when necessary; other converted values are stored only when necessary to avoid data loss. The model must accurately distinguish known information from missing information.

The profile stores one optional energy amount, retaining the selected source unit. The USDA adapter chooses supplied values in this project-defined order: Atwater specific factors (2048, kcal), Atwater general factors (2047, kcal), energy (1008, kcal), then energy (1062, kJ). This prefers food-specific factors, then general factors, then older unspecified kcal, and uses supplied kJ when no kcal candidate exists. Missing candidates fall through; invalid candidate data is not silently masked. A requested unit conversion uses the selected amount without storing a derived duplicate.
