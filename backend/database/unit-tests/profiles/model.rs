use std::error::Error;
use std::str::FromStr;

use chrono::{NaiveDate, TimeZone, Utc};
use rust_decimal::Decimal;

use crate::nutrition_model::{Kcal, Kilograms, Measurement, PerDay};
use crate::profiles::model::ProfileData;
use crate::profiles::*;

#[test]
fn profile_names_use_canonical_unicode_caseless_keys_and_keep_trimmed_spelling()
-> Result<(), Box<dyn Error>> {
    assert_eq!(ProfileName::new("Straße")?, ProfileName::new("STRASSE")?);
    assert_eq!(ProfileName::new("Σ")?, ProfileName::new("ς")?);
    assert_eq!(ProfileName::new("é")?, ProfileName::new("e\u{0301}")?);
    assert_ne!(ProfileName::new("é")?, ProfileName::new("e")?);
    assert_eq!(ProfileName::new("  Alex  ")?.as_str(), "Alex");
    assert!(ProfileName::new(" \t\n").is_err());
    Ok(())
}

#[test]
fn profile_ids_and_birth_dates_validate_and_preserve_wire_precision() -> Result<(), Box<dyn Error>>
{
    assert_eq!(serde_json::to_string(&ProfileId::new(12)?)?, "12");
    assert!(serde_json::from_str::<ProfileId>("0").is_err());
    assert!(serde_json::from_str::<ProfileId>("-2").is_err());

    for (value, precision, effective) in [
        ("0001", BirthDatePrecision::Year, "0001-01-01"),
        ("1982", BirthDatePrecision::Year, "1982-01-01"),
        ("1982-09", BirthDatePrecision::YearMonth, "1982-09-01"),
        ("1982-09-10", BirthDatePrecision::FullDate, "1982-09-10"),
        ("2000-02-29", BirthDatePrecision::FullDate, "2000-02-29"),
        ("9999-12-31", BirthDatePrecision::FullDate, "9999-12-31"),
    ] {
        let date: BirthDate = serde_json::from_str(&format!("\"{value}\""))?;
        assert_eq!(serde_json::to_string(&date)?, format!("\"{value}\""));
        assert_eq!(date.precision(), precision);
        assert_eq!(date.effective_date().to_string(), effective);
    }
    for value in [
        "0000",
        "0000-01",
        "0000-01-01",
        "1982-00",
        "1982-13",
        "1982-02-29",
        "1982-09-00",
        "+982-09",
        "+982-09-10",
        "1982-9",
        "1982-09-1",
        "1982-09-010",
        "１９８２",
        "-001",
        "1982-09-10\n",
    ] {
        assert!(
            BirthDate::parse(value).is_err(),
            "accepted invalid date {value}"
        );
        assert!(serde_json::from_value::<BirthDate>(serde_json::json!(value)).is_err());
    }
    Ok(())
}

#[test]
fn create_and_patch_inputs_reject_unknown_fields_and_keep_null_semantics()
-> Result<(), Box<dyn Error>> {
    let create: CreateProfile = serde_json::from_str(
        r#"{"name":" Alex ","settings":{"birth_date":"1982","timezone":"UTC"}}"#,
    )?;
    assert_eq!(create.name.as_str(), "Alex");
    assert!(serde_json::from_str::<CreateProfile>(r#"{"name":"Alex","user_id":2}"#).is_err());
    assert!(serde_json::from_str::<CreateProfile>(r#"{"name":"Alex","cache":{}}"#).is_err());

    let patch: PatchProfile =
        serde_json::from_str(r#"{"name":"Alex","birth_date":null,"weight_goal":null,"bmr":null}"#)?;
    assert!(matches!(patch.name, Patch::Set(_)));
    assert_eq!(patch.birth_date, NullablePatch::Clear);
    assert_eq!(patch.weight_goal, NullablePatch::Clear);
    assert_eq!(patch.bmr, BmrPatch::ResetToCalculated);
    assert_eq!(patch.sex, NullablePatch::Unchanged);
    assert!(serde_json::from_str::<PatchProfile>(r#"{"name":null}"#).is_err());
    assert!(serde_json::from_str::<PatchProfile>(r#"{"timezone":null}"#).is_err());
    assert!(serde_json::from_str::<PatchProfile>(r#"{"derived":{}}"#).is_err());

    let custom_bmr: BmrSetting =
        serde_json::from_str(r#"{"mode":"custom","value":"1200 kcal/day"}"#)?;
    assert!(matches!(custom_bmr, BmrSetting::Custom { .. }));
    assert!(
        serde_json::from_str::<BmrSetting>(r#"{"mode":"custom","value":"0 kcal/day"}"#).is_err()
    );
    assert!(
        serde_json::from_str::<BmrSetting>(
            r#"{"mode":"custom","value":"1200 kcal/day","cache":null}"#
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn calculated_bmr_rejects_extra_fields_directly_and_in_create_or_patch()
-> Result<(), Box<dyn Error>> {
    let calculated = serde_json::json!({"mode": "calculated"});
    assert_eq!(
        serde_json::from_value::<BmrSetting>(calculated.clone())?,
        BmrSetting::Calculated
    );
    assert_eq!(serde_json::to_value(BmrSetting::Calculated)?, calculated);
    assert_eq!(
        serde_json::from_value::<CreateProfile>(serde_json::json!({
            "name": "Alex",
            "settings": {"bmr": calculated}
        }))?
        .settings
        .and_then(|settings| settings.bmr),
        Some(BmrSetting::Calculated)
    );
    assert_eq!(
        serde_json::from_value::<PatchProfile>(serde_json::json!({"bmr": calculated}))?.bmr,
        BmrPatch::Set(BmrSetting::Calculated)
    );

    for bmr in [
        serde_json::json!({"mode": "calculated", "value": "100 kcal/day"}),
        serde_json::json!({"mode": "calculated", "current_weight": "70 kg"}),
        serde_json::json!({"mode": "calculated", "unexpected": true}),
    ] {
        assert!(serde_json::from_value::<BmrSetting>(bmr.clone()).is_err());
        assert!(
            serde_json::from_value::<CreateProfile>(serde_json::json!({
                "name": "Alex",
                "settings": {"bmr": bmr}
            }))
            .is_err()
        );
        assert!(serde_json::from_value::<PatchProfile>(serde_json::json!({"bmr": bmr})).is_err());
    }

    let custom_json = serde_json::json!({"mode": "custom", "value": "1200 kcal/day"});
    let custom: BmrSetting = serde_json::from_value(custom_json.clone())?;
    assert!(matches!(custom, BmrSetting::Custom { .. }));
    assert_eq!(serde_json::to_value(custom)?, custom_json);
    Ok(())
}

#[test]
fn calorie_values_keep_fractional_results_and_custom_baseline_is_independent()
-> Result<(), Box<dyn Error>> {
    let current_weight = BodyWeight::new(Kilograms::from_str("70 kg")?)?;
    let height = BodyHeight::new(crate::nutrition_model::Centimetres::from_str("170 cm")?)?;
    let birth_date = BirthDate::full_date(1990, 1, 1)?;
    let current_day = NaiveDate::from_ymd_opt(2020, 1, 1)
        .ok_or_else(|| std::io::Error::other("invalid test date"))?;
    let now = Utc
        .from_utc_datetime(
            &current_day
                .and_hms_opt(12, 0, 0)
                .ok_or_else(|| std::io::Error::other("invalid test time"))?,
        )
        .timestamp_millis();
    let mut settings = ProfileSettings {
        birth_date: Some(birth_date),
        sex: Some(Sex::Male),
        height: Some(height),
        baseline_activity: Some(BaselineActivity::Preset {
            level: ActivityLevel::LightlyActive,
        }),
        ..ProfileSettings::default()
    };
    let cache = ProfileCache {
        current_weight: Some(current_weight),
    };

    let derived = ProfileDerived::calculate(&settings, &cache, now)?;
    assert_eq!(
        derived.bmr().map(|value| value.amount().value()),
        Some(Decimal::from_str("1617.5")?)
    );
    assert_eq!(
        derived
            .baseline_activity()
            .map(|value| value.amount().value()),
        Some(Decimal::from_str("606.5625")?)
    );
    assert_eq!(
        derived
            .maintenance_calories()
            .map(|value| value.amount().value()),
        Some(Decimal::from_str("2224.0625")?)
    );

    settings.birth_date = None;
    settings.sex = None;
    settings.height = None;
    settings.bmr = BmrSetting::Calculated;
    settings.baseline_activity = Some(BaselineActivity::Custom {
        value: PerDay::<Kcal>::from_str("50.25 kcal/day")?,
    });
    let custom_baseline = ProfileDerived::calculate(&settings, &ProfileCache::default(), now)?;
    assert_eq!(custom_baseline.bmr(), None);
    assert_eq!(
        custom_baseline
            .baseline_activity()
            .map(|value| value.amount().value()),
        Some(Decimal::from_str("50.25")?)
    );
    assert_eq!(custom_baseline.maintenance_calories(), None);

    settings.bmr = BmrSetting::Custom {
        value: CustomBmr::new(PerDay::<Kcal>::from_str("1000.5 kcal/day")?)?,
    };
    let custom = ProfileDerived::calculate(&settings, &ProfileCache::default(), now)?;
    assert_eq!(
        custom.bmr().map(|value| value.amount().value()),
        Some(Decimal::from_str("1000.5")?)
    );
    assert_eq!(
        custom
            .maintenance_calories()
            .map(|value| value.amount().value()),
        Some(Decimal::from_str("1050.75")?)
    );
    Ok(())
}

#[test]
fn baseline_multiplication_uses_decimal_precision_rounding() -> Result<(), Box<dyn Error>> {
    let settings = ProfileSettings {
        bmr: BmrSetting::Custom {
            value: CustomBmr::new(PerDay::<Kcal>::from_str(
                "1000.0000000000000000000000001 kcal/day",
            )?)?,
        },
        baseline_activity: Some(BaselineActivity::Preset {
            level: ActivityLevel::LightlyActive,
        }),
        ..ProfileSettings::default()
    };

    let derived = ProfileDerived::calculate(&settings, &ProfileCache::default(), 1_000)?;
    assert_eq!(
        derived.baseline_activity().map(|value| value.to_string()),
        Some("375.00000000000000000000000004 kcal/day".to_owned())
    );
    Ok(())
}

#[test]
fn presets_use_selected_baseline_coefficients_and_timezone_local_age() -> Result<(), Box<dyn Error>>
{
    let current_weight = BodyWeight::new(Kilograms::from_str("70 kg")?)?;
    let height = BodyHeight::new(crate::nutrition_model::Centimetres::from_str("170 cm")?)?;
    let birth_date = BirthDate::year(1990)?;
    let instant = chrono::DateTime::parse_from_rfc3339("2020-01-01T00:30:00Z")?.timestamp_millis();
    let mut settings = ProfileSettings {
        birth_date: Some(birth_date),
        sex: Some(Sex::Male),
        height: Some(height),
        timezone: IanaTimezone::new("America/Los_Angeles")?,
        ..ProfileSettings::default()
    };
    let cache = ProfileCache {
        current_weight: Some(current_weight),
    };

    settings.baseline_activity = Some(BaselineActivity::Preset {
        level: ActivityLevel::NoActivity,
    });
    let no_activity = ProfileDerived::calculate(&settings, &cache, instant)?;
    assert_eq!(
        no_activity.bmr().map(|value| value.amount().value()),
        Some(Decimal::from_str("1622.5")?)
    );
    assert_eq!(
        no_activity
            .baseline_activity()
            .map(|value| value.amount().value()),
        Some(Decimal::ZERO)
    );

    for (level, expected) in [
        (ActivityLevel::Sedentary, "324.5"),
        (ActivityLevel::LightlyActive, "608.4375"),
        (ActivityLevel::ModeratelyActive, "811.25"),
        (ActivityLevel::VeryActive, "1460.25"),
    ] {
        settings.baseline_activity = Some(BaselineActivity::Preset { level });
        let derived = ProfileDerived::calculate(&settings, &cache, instant)?;
        assert_eq!(
            derived
                .baseline_activity()
                .map(|value| value.amount().value()),
            Some(Decimal::from_str(expected)?)
        );
    }
    Ok(())
}

#[test]
fn future_birth_dates_keep_baseline_and_clock_semantics() -> Result<(), Box<dyn Error>> {
    let current_weight = BodyWeight::new(Kilograms::from_str("70 kg")?)?;
    let height = BodyHeight::new(crate::nutrition_model::Centimetres::from_str("170 cm")?)?;
    let future_birth_date = BirthDate::full_date(2027, 1, 1)?;
    let now = chrono::DateTime::parse_from_rfc3339("2026-01-01T12:00:00Z")?.timestamp_millis();
    let cache = ProfileCache {
        current_weight: Some(current_weight),
    };
    let mut settings = ProfileSettings {
        birth_date: Some(future_birth_date),
        sex: Some(Sex::Male),
        height: Some(height),
        ..ProfileSettings::default()
    };

    for (baseline_activity, expected_baseline) in [
        (None, None),
        (
            Some(BaselineActivity::Preset {
                level: ActivityLevel::LightlyActive,
            }),
            None,
        ),
        (
            Some(BaselineActivity::Custom {
                value: PerDay::<Kcal>::from_str("0 kcal/day")?,
            }),
            Some(Decimal::ZERO),
        ),
    ] {
        settings.baseline_activity = baseline_activity;
        let derived = ProfileDerived::calculate(&settings, &cache, now)?;
        assert_eq!(derived.bmr(), None);
        assert_eq!(
            derived
                .baseline_activity()
                .map(|value| value.amount().value()),
            expected_baseline
        );
        assert_eq!(derived.maintenance_calories(), None);
    }

    settings.birth_date = None;
    settings.sex = None;
    settings.height = None;
    settings.bmr = BmrSetting::Custom {
        value: CustomBmr::new(PerDay::<Kcal>::from_str("1200 kcal/day")?)?,
    };
    settings.baseline_activity = Some(BaselineActivity::Preset {
        level: ActivityLevel::Sedentary,
    });
    let custom_bmr = ProfileDerived::calculate(&settings, &ProfileCache::default(), now)?;
    assert_eq!(
        custom_bmr.bmr().map(|value| value.amount().value()),
        Some(Decimal::from(1200))
    );
    assert_eq!(
        custom_bmr
            .baseline_activity()
            .map(|value| value.amount().value()),
        Some(Decimal::from(240))
    );
    assert_eq!(
        custom_bmr
            .maintenance_calories()
            .map(|value| value.amount().value()),
        Some(Decimal::from(1440))
    );

    assert!(matches!(
        ProfileDerived::calculate(&ProfileSettings::default(), &ProfileCache::default(), i64::MAX),
        Err(ProfileError::Calculation(message)) if message == "clock timestamp is out of range"
    ));
    Ok(())
}

#[test]
fn profile_data_response_cannot_be_used_as_a_create_payload() -> Result<(), Box<dyn Error>> {
    let response = ProfileData {
        settings: ProfileSettings::default(),
        cache: ProfileCache::default(),
    };
    let encoded = serde_json::to_string(&response)?;
    assert!(encoded.contains("\"current_weight\":null"));
    assert!(
        serde_json::from_str::<CreateProfile>(&format!(
            "{{\"name\":\"Alex\",\"settings\":{encoded}}}"
        ))
        .is_err()
    );
    Ok(())
}

#[test]
fn derived_values_validate_components_and_deserialization() -> Result<(), Box<dyn Error>> {
    let bmr = PerDay::<Kcal>::from_str("1200 kcal/day")?;
    let activity = PerDay::<Kcal>::from_str("300 kcal/day")?;
    let derived = ProfileDerived::from_components(Some(bmr), Some(activity));
    assert_eq!(
        derived
            .maintenance_calories()
            .map(|value| value.to_string()),
        Some("1500 kcal/day".to_owned())
    );
    let encoded = serde_json::to_string(&derived)?;
    assert_eq!(serde_json::from_str::<ProfileDerived>(&encoded)?, derived);

    let incomplete = ProfileDerived::from_components(None, Some(activity));
    assert_eq!(incomplete.maintenance_calories(), None);
    assert_eq!(
        serde_json::from_str::<ProfileDerived>(&serde_json::to_string(&incomplete)?)?,
        incomplete
    );

    for input in [
        r#"{"bmr":null,"baseline_activity":null,"maintenance_calories":"1 kcal/day"}"#,
        r#"{"bmr":"1200 kcal/day","baseline_activity":"300 kcal/day","maintenance_calories":null}"#,
        r#"{"bmr":"1200 kcal/day","baseline_activity":"300 kcal/day","maintenance_calories":"1499 kcal/day"}"#,
        r#"{"bmr":"1200 kcal/day","baseline_activity":"300 kcal/day"}"#,
    ] {
        assert!(serde_json::from_str::<ProfileDerived>(input).is_err());
    }

    Ok(())
}

#[test]
fn custom_rates_validate_domain_invariants() -> Result<(), Box<dyn Error>> {
    let negative = PerDay::<Kcal>::from_str("-1 kcal/day")?;
    let zero = PerDay::<Kcal>::from_str("0 kcal/day")?;
    assert!(CustomBmr::new(negative).is_err());
    assert!(CustomBmr::new(zero).is_err());
    assert!(
        serde_json::from_str::<BaselineActivity>(r#"{"mode":"custom","value":"-1 kcal/day"}"#)
            .is_ok()
    );
    assert!(
        serde_json::from_str::<BmrSetting>(r#"{"mode":"custom","value":"-1 kcal/day"}"#).is_err()
    );
    Ok(())
}

#[test]
fn derived_values_preserve_signed_arithmetic() -> Result<(), Box<dyn Error>> {
    let bmr = PerDay::<Kcal>::from_str("-20 kcal/day")?;
    let activity = PerDay::<Kcal>::from_str("5 kcal/day")?;
    let result = ProfileDerived::from_components(Some(bmr), Some(activity));
    assert_eq!(
        result.maintenance_calories().map(|value| value.to_string()),
        Some("-15 kcal/day".to_owned())
    );
    Ok(())
}

#[test]
fn body_measurements_reject_zero_and_negative_quantities() -> Result<(), Box<dyn Error>> {
    use crate::nutrition_model::Centimetres;

    for quantity in ["0 cm", "-1 cm"] {
        let height = Centimetres::from_str(quantity)?;
        assert!(BodyHeight::new(height).is_err());
        assert!(serde_json::from_value::<BodyHeight>(serde_json::json!(quantity)).is_err());
    }
    for quantity in ["0 kg", "-1 kg"] {
        let weight = Kilograms::from_str(quantity)?;
        assert!(BodyWeight::new(weight).is_err());
        assert!(serde_json::from_value::<BodyWeight>(serde_json::json!(quantity)).is_err());
    }
    Ok(())
}

#[test]
fn create_and_patch_reject_bare_energy_for_daily_rates() {
    for settings in [
        serde_json::json!({"bmr": {"mode": "custom", "value": "1200 kcal"}}),
        serde_json::json!({"baseline_activity": {"mode": "custom", "value": "300 kcal"}}),
    ] {
        let create = serde_json::json!({"name": "Alex", "settings": settings.clone()});
        assert!(serde_json::from_value::<CreateProfile>(create).is_err());
        assert!(serde_json::from_value::<PatchProfile>(settings).is_err());
    }
}

#[test]
fn weekly_weight_loss_validates_units_and_calculates_deficit() -> Result<(), Box<dyn Error>> {
    let half: WeeklyWeightLoss = serde_json::from_str("\"0.5 kg/week\"")?;
    assert_eq!(half.daily_calorie_deficit().value(), Decimal::from(550));
    assert_eq!(WeeklyWeightLoss::new("0.5 kg/week".parse()?)?, half);
    assert!(WeeklyWeightLoss::new("-0.5 kg/week".parse()?).is_err());
    assert_eq!(serde_json::to_string(&half)?, "\"0.5 kg/week\"");
    let precise: WeeklyWeightLoss = serde_json::from_str("\"0.123 kg/week\"")?;
    assert_eq!(
        precise.daily_calorie_deficit().value(),
        Decimal::from_str_exact("135.3")?
    );
    let zero: WeeklyWeightLoss = serde_json::from_str("\"0 kg/week\"")?;
    assert_eq!(zero.daily_calorie_deficit().value(), Decimal::ZERO);
    for value in ["-0.5 kg/week", "0.5 kg/day", "0.5 kg", "0.5"] {
        assert!(serde_json::from_value::<WeeklyWeightLoss>(serde_json::json!(value)).is_err());
        assert!(
            serde_json::from_value::<CreateProfile>(
                serde_json::json!({"name":"Alex","settings":{"weekly_weight_loss":value}})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<PatchProfile>(serde_json::json!({"weekly_weight_loss":value}))
                .is_err()
        );
    }
    let legacy: ProfileSettings = serde_json::from_value(serde_json::json!({}))?;
    assert_eq!(legacy.weekly_weight_loss, None);
    let unset_derived = ProfileDerived::calculate(&legacy, &ProfileCache::default(), 1_000)?;
    assert_eq!(unset_derived.daily_calorie_deficit(), None);
    let zero_settings: ProfileSettings =
        serde_json::from_value(serde_json::json!({"weekly_weight_loss": "0 kg/week"}))?;
    let zero_derived = ProfileDerived::calculate(&zero_settings, &ProfileCache::default(), 1_000)?;
    assert_eq!(
        zero_derived
            .daily_calorie_deficit()
            .map(|value| value.value()),
        Some(Decimal::ZERO)
    );
    for settings in [
        serde_json::json!({}),
        serde_json::json!({"weekly_weight_loss": null}),
    ] {
        let create: CreateProfile =
            serde_json::from_value(serde_json::json!({"name": "Alex", "settings": settings}))?;
        assert_eq!(
            create
                .settings
                .ok_or("missing settings")?
                .weekly_weight_loss,
            None
        );
    }
    let clear: PatchProfile = serde_json::from_str(r#"{"weekly_weight_loss":null}"#)?;
    assert_eq!(clear.weekly_weight_loss, NullablePatch::Clear);
    let keep: PatchProfile = serde_json::from_str("{}")?;
    assert_eq!(keep.weekly_weight_loss, NullablePatch::Unchanged);
    let settings: ProfileSettings =
        serde_json::from_value(serde_json::json!({"weekly_weight_loss":"0.5 kg/week"}))?;
    assert_eq!(
        serde_json::from_value::<ProfileSettings>(serde_json::to_value(&settings)?)?,
        settings
    );
    let derived = ProfileDerived::calculate(&settings, &ProfileCache::default(), 1_000)?;
    assert_eq!(derived.bmr(), None);
    assert_eq!(derived.maintenance_calories(), None);
    assert_eq!(
        derived.daily_calorie_deficit().map(|value| value.value()),
        Some(Decimal::from(550))
    );
    assert_eq!(
        serde_json::from_value::<ProfileDerived>(serde_json::to_value(derived)?)?,
        derived
    );
    let legacy_derived: ProfileDerived = serde_json::from_str(
        r#"{"bmr":null,"baseline_activity":null,"maintenance_calories":null}"#,
    )?;
    assert_eq!(legacy_derived.daily_calorie_deficit(), None);
    assert!(serde_json::from_str::<ProfileDerived>(r#"{"bmr":null,"baseline_activity":null,"maintenance_calories":null,"daily_calorie_deficit":"-1 kcal/day"}"#).is_err());
    Ok(())
}
