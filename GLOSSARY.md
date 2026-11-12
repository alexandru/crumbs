# Crumbs

Crumbs manages food information from multiple sources.

## Language

**Food**:
A food entry supplied by a source, whether an ordinary ingredient or a branded product.

**Source**:
The provider of a food entry, such as USDA FoodData Central.

**Dataset**:
A source-independent search classification: common for generic reference foods, branded for imported products, and custom for foods managed directly in Crumbs. Each food belongs to one dataset.

**Custom food**:
A food created and managed directly in Crumbs. It may include nutrition information and notes such as ingredients.

**Source food ID**:
The identifier a source assigns to a food entry. It identifies the entry within that source, not across different sources.

**Nutrition basis**:
The measured food quantity to which a nutrition profile applies: 100 grams or 100 millilitres.

**Serving**:
A named portion of a food with a known measured mass or volume.

**Nutrition profile**:
The reported nutrient amounts for 100 grams or 100 millilitres of a food.

**Consumed nutrition**:
Nutrient amounts for the quantity of food recorded in a food log entry. These amounts retain the values used when the entry was recorded.

**Reported nutrient amount**:
An amount of a nutrient for a nutrition basis, including any stated comparison or approximation.

**Carbohydrates**:
Food carbohydrates whose inclusion of fibre depends on the nutrition convention.

**Fibre**:
Dietary fibre, which may be included in carbohydrate totals depending on the nutrition convention.

**European nutrition label**:
A nutrition label whose carbohydrates exclude fibre, which is reported separately.

**American nutrition label**:
A nutrition label whose total carbohydrates include fibre.

**Tag**:
A reusable label for foods. Tag names are case-insensitive, ignore surrounding whitespace, and retain meaningful accents.

**Tag assignment**:
The association of a food with a tag. Assignments are managed by Crumbs and survive updates from the food's source.

**Profile**:
The personal context for one person. One user can have multiple profiles, whose names are unique within that user after ignoring case and surrounding whitespace.

**Profile log entry**:
A record of food consumed, a biometric measurement, or a journal entry for one profile.

**Journal entry**:
A free-text record of a person's feelings, experiences, or observations for one profile. A person can record multiple journal entries on the same day.

**Biometric measurement**:
A recorded value for a physical characteristic of a person, such as body weight.

**Food log entry**:
A record of food consumed, its quantity, calories and nutrition information, and a description retained from when it was recorded.

**Current weight**:
The weight from a profile’s most recent recorded weight measurement. It is unknown when the profile has no weight measurements.

**Weight goal**:
An optional body weight that a person wants to reach.

**Weekly weight loss**:
The body mass that a person wants to lose each week. A zero rate means no requested weight loss.

**Daily calorie deficit**:
The estimated daily reduction below maintenance calories for a person's requested weekly weight loss.

**Basal metabolic rate (BMR)**:
The estimated daily energy expenditure at rest, calculated from profile information or supplied as a custom value.

**Maintenance calories**:
The estimated daily energy intake needed to maintain a person’s current weight at their baseline activity level.

**Birth date**:
A person’s date of birth, recorded to the nearest year, month, or day. Missing month and day components are treated as January and the first day when a complete date is needed.

**Baseline activity level**:
A person's ordinary activity outside separately recorded activity, represented by a preset multiplier or a custom fixed daily energy addition.

**No activity**:
The baseline activity preset that adds no energy above basal metabolic rate.
