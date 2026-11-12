# Profile API

Profiles contain personal settings and derived calorie values. Each profile belongs to one authenticated account.

## Weekly weight loss

The optional `weekly_weight_loss` setting records the requested weight loss in kilograms per week. It is independent of `weight_goal`. Use an exact decimal string with the `kg/week` unit, such as `"0.5 kg/week"`.

Create a profile through `POST /profiles`:

```json
{
  "name": "Alex",
  "settings": {
    "weight_goal": "70 kg",
    "weekly_weight_loss": "0.5 kg/week"
  }
}
```

The rate must be zero or positive. Negative rates and incompatible units are invalid. A zero rate requests no deficit. Omission or null leaves the rate unset.

Change the rate through `PATCH /profiles/{profile_id}`:

```json
{
  "weekly_weight_loss": "0.25 kg/week"
}
```

Omission preserves the current rate. Null clears it. A rate change does not change the target weight, cached current weight, BMR, or maintenance calories.

MCP uses the same shapes: `create_profile` accepts the create object inside `profile`; `update_profile` accepts the patch inside `patch`, with `id` identifying the profile. `get_profile` and `list_profiles` return the stored rate and derived deficit.

## Daily calorie deficit

Profile responses include `data.settings.weekly_weight_loss` and `derived.daily_calorie_deficit`. The deficit is calculated from the current rate whenever the profile is returned:

```text
daily deficit = weekly loss × 7,700 kcal/kg ÷ 7 days
0.5 kg/week   → 550 kcal/day
0.25 kg/week  → 275 kcal/day
0 kg/week     → 0 kcal/day
unset        → null
```

The calculation uses fixed-point decimals. The response value includes its unit, for example `"550 kcal/day"`. Decimal scale can be retained, so `"550.0 kcal/day"` represents the same amount.

The deficit remains available when other profile inputs are incomplete. Maintenance calories keep their existing meaning: estimated energy needed to maintain current weight at baseline activity. The deficit is a separate planning value.

The fixed conversion is an estimate. It does not predict actual weight change or account for metabolic adaptation. See [NIDDK's explanation of fixed calorie-to-weight rules](https://www.niddk.nih.gov/research-funding/technology-advancement-transfer/research-materials-licensing/body-weight-simulator-java-applet).

## Existing profiles

Profiles created before the weekly rate was supported load with an unset rate and a null daily deficit. This setting uses existing profile JSON storage and requires no database migration.
