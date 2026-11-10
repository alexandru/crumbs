use std::error::Error;
use std::str::FromStr;

use chrono::{NaiveDate, TimeZone, Utc};
use rust_decimal::Decimal;

use crate::nutrition_model::Kilograms;
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

    for value in ["1982", "1982-09", "1982-09-10"] {
        let date: BirthDate = serde_json::from_str(&format!("\"{value}\""))?;
        assert_eq!(serde_json::to_string(&date)?, format!("\"{value}\""));
    }
    for value in [
        "0000",
        "1982-00",
        "1982-13",
        "1982-02-29",
        "1982-09-00",
        "+982-09",
        "+982-09-10",
        "1982-9",
        "1982-09-1",
        "1982-09-010",
    ] {
        assert!(
            serde_json::from_str::<BirthDate>(&format!("\"{value}\"")).is_err(),
            "accepted invalid date {value}"
        );
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

    let custom_bmr: BmrSetting = serde_json::from_str(r#"{"mode":"custom","value":"1200 kcal"}"#)?;
    assert!(matches!(custom_bmr, BmrSetting::Custom { .. }));
    assert!(serde_json::from_str::<BmrSetting>(r#"{"mode":"custom","value":"0 kcal"}"#).is_err());
    assert!(
        serde_json::from_str::<BmrSetting>(r#"{"mode":"custom","value":"1200 kcal","cache":null}"#)
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
        serde_json::json!({"mode": "calculated", "value": "100 kcal"}),
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

    let custom_json = serde_json::json!({"mode": "custom", "value": "1200 kcal"});
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
        derived.bmr().map(|value| value.value()),
        Some(Decimal::from_str("1617.5")?)
    );
    assert_eq!(
        derived.baseline_activity().map(|value| value.value()),
        Some(Decimal::from_str("606.5625")?)
    );
    assert_eq!(
        derived.maintenance_calories().map(|value| value.value()),
        Some(Decimal::from_str("2224.0625")?)
    );

    settings.birth_date = None;
    settings.sex = None;
    settings.height = None;
    settings.bmr = BmrSetting::Calculated;
    settings.baseline_activity = Some(BaselineActivity::Custom {
        value: KilocaloriesPerDay::from_str("50.25 kcal")?,
    });
    let custom_baseline = ProfileDerived::calculate(&settings, &ProfileCache::default(), now)?;
    assert_eq!(custom_baseline.bmr(), None);
    assert_eq!(
        custom_baseline
            .baseline_activity()
            .map(|value| value.value()),
        Some(Decimal::from_str("50.25")?)
    );
    assert_eq!(custom_baseline.maintenance_calories(), None);

    settings.bmr = BmrSetting::Custom {
        value: CustomBmr::new(KilocaloriesPerDay::from_str("1000.5 kcal")?)?,
    };
    let custom = ProfileDerived::calculate(&settings, &ProfileCache::default(), now)?;
    assert_eq!(
        custom.bmr().map(|value| value.value()),
        Some(Decimal::from_str("1000.5")?)
    );
    assert_eq!(
        custom.maintenance_calories().map(|value| value.value()),
        Some(Decimal::from_str("1050.75")?)
    );
    Ok(())
}

#[test]
fn baseline_multiplication_uses_decimal_precision_rounding() -> Result<(), Box<dyn Error>> {
    let settings = ProfileSettings {
        bmr: BmrSetting::Custom {
            value: CustomBmr::new(KilocaloriesPerDay::from_str(
                "1000.0000000000000000000000001 kcal",
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
        Some("375.00000000000000000000000004 kcal".to_owned())
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
        no_activity.bmr().map(|value| value.value()),
        Some(Decimal::from_str("1622.5")?)
    );
    assert_eq!(
        no_activity.baseline_activity().map(|value| value.value()),
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
            derived.baseline_activity().map(|value| value.value()),
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
                value: KilocaloriesPerDay::from_str("0 kcal")?,
            }),
            Some(Decimal::ZERO),
        ),
    ] {
        settings.baseline_activity = baseline_activity;
        let derived = ProfileDerived::calculate(&settings, &cache, now)?;
        assert_eq!(derived.bmr(), None);
        assert_eq!(
            derived.baseline_activity().map(|value| value.value()),
            expected_baseline
        );
        assert_eq!(derived.maintenance_calories(), None);
    }

    settings.birth_date = None;
    settings.sex = None;
    settings.height = None;
    settings.bmr = BmrSetting::Custom {
        value: CustomBmr::new(KilocaloriesPerDay::from_str("1200 kcal")?)?,
    };
    settings.baseline_activity = Some(BaselineActivity::Preset {
        level: ActivityLevel::Sedentary,
    });
    let custom_bmr = ProfileDerived::calculate(&settings, &ProfileCache::default(), now)?;
    assert_eq!(
        custom_bmr.bmr().map(|value| value.value()),
        Some(Decimal::from(1200))
    );
    assert_eq!(
        custom_bmr.baseline_activity().map(|value| value.value()),
        Some(Decimal::from(240))
    );
    assert_eq!(
        custom_bmr.maintenance_calories().map(|value| value.value()),
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
    let bmr = KilocaloriesPerDay::from_str("1200 kcal")?;
    let activity = KilocaloriesPerDay::from_str("300 kcal")?;
    let derived = ProfileDerived::from_components(Some(bmr), Some(activity))?;
    assert_eq!(
        derived
            .maintenance_calories()
            .map(|value| value.to_string()),
        Some("1500 kcal".to_owned())
    );
    let encoded = serde_json::to_string(&derived)?;
    assert_eq!(serde_json::from_str::<ProfileDerived>(&encoded)?, derived);

    let incomplete = ProfileDerived::from_components(None, Some(activity))?;
    assert_eq!(incomplete.maintenance_calories(), None);
    assert_eq!(
        serde_json::from_str::<ProfileDerived>(&serde_json::to_string(&incomplete)?)?,
        incomplete
    );

    for input in [
        r#"{"bmr":null,"baseline_activity":null,"maintenance_calories":"1 kcal"}"#,
        r#"{"bmr":"1200 kcal","baseline_activity":"300 kcal","maintenance_calories":null}"#,
        r#"{"bmr":"1200 kcal","baseline_activity":"300 kcal","maintenance_calories":"1499 kcal"}"#,
        r#"{"bmr":"1200 kcal","baseline_activity":"300 kcal"}"#,
    ] {
        assert!(serde_json::from_str::<ProfileDerived>(input).is_err());
    }

    let maximum = KilocaloriesPerDay::from_str("79228162514264337593543950335 kcal")?;
    assert!(matches!(
        ProfileDerived::from_components(Some(maximum), Some(activity)),
        Err(ProfileError::Calculation(_))
    ));
    Ok(())
}
