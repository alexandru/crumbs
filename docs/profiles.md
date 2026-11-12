# Profile API

Profiles contain personal settings and derived calorie values. Each profile belongs to one authenticated account.

## Weight change goal

The optional `weight_change_goal` setting records the requested weight change in kilograms per week. It is independent of `weight_goal`. Use an exact decimal string with the `kg/week` unit, such as `"-0.5 kg/week"` for weight loss or `"0.5 kg/week"` for weight gain.

Create a profile through `POST /profiles`:

```json
{
  "name": "Alex",
  "settings": {
    "weight_goal": "70 kg",
    "weight_change_goal": "-0.5 kg/week"
  }
}
```

Negative values request weight loss, positive values request weight gain, and zero requests no weight change. Incompatible units are invalid. Omission or null leaves the goal unset.

Change the rate through `PATCH /profiles/{profile_id}`:

```json
{
  "weight_change_goal": "-0.25 kg/week"
}
```

Omission preserves the current goal. Null clears it. A weight change goal does not change the target weight, cached current weight, BMR, or maintenance calories.

MCP uses the same shapes: `create_profile` accepts the create object inside `profile`; `update_profile` accepts the patch inside `patch`, with `id` identifying the profile. `get_profile` and `list_profiles` return the stored weight change goal and derived calorie balance goal.

Each profile tool publishes self-contained input and output JSON Schemas. Output schemas describe profile results and structured errors.

## Calorie balance goal

Profile responses include `data.settings.weight_change_goal` and `derived.calorie_balance_goal`. The balance is calculated from the current weight change goal whenever the profile is returned:

```text
calorie balance goal = weight change goal × 7,700 kcal/kg ÷ 7 days
-0.5 kg/week  → -550 kcal/day  (deficit)
0.5 kg/week   → 550 kcal/day   (surplus)
0 kg/week     → 0 kcal/day
unset         → null
```

The calculation uses fixed-point decimals. The response value includes its unit, for example `"-550 kcal/day"`. Decimal scale can be retained, so `"-550.0 kcal/day"` represents the same amount.

The calorie balance goal remains available when other profile inputs are incomplete. Maintenance calories keep their existing meaning: estimated energy needed to maintain current weight at baseline activity. The calorie balance goal is a separate planning value; add it to maintenance calories to calculate a calorie intake target.

The fixed conversion is an estimate. It does not predict actual weight change or account for metabolic adaptation. See [NIDDK's explanation of fixed calorie-to-weight rules](https://www.niddk.nih.gov/research-funding/technology-advancement-transfer/research-materials-licensing/body-weight-simulator-java-applet).

## Existing profiles

Profiles created before the weight change goal was supported load with an unset goal and a null calorie balance goal. This setting uses existing profile JSON storage and requires no database migration.
