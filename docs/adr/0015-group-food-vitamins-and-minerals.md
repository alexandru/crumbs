---
status: accepted
---

# Group food vitamins and minerals

Food nutrition uses named `minerals` and `vitamins` groups. Water, caffeine, and choline are direct nutrition fields. Short nutrient names identify the nutrient; documentation defines its measured form. For example, `vitamins.vitamin_a` means vitamin A in retinol activity equivalents. This keeps the public names short without treating different nutrient forms as equivalent.

The mineral group contains calcium, iron, magnesium, phosphorus, potassium, zinc, copper, manganese, and selenium. The vitamin group contains A, C, D, E, K1, B1, B2, B3, B5, B6, B9, and B12. Existing nutrient fields keep their locations, including sodium. All added amounts use the existing reported mass type, with explicit units and source qualifiers. Missing groups and amounts mean unknown.

The shared model applies to imported foods, custom foods, and consumed nutrition. USDA imports use the specific nutrient forms documented in the [food API](../foods.md#nutrition). Reimport updates existing foods through their source identities. Existing food-log snapshots retain their recorded amounts.
