use std::error::Error;

use rust_decimal::Decimal;

use crate::profile_logs::*;
use crate::profiles::{BodyWeight, Patch};

#[test]
fn log_day_validates_calendar_days_and_preserves_iso_form() -> Result<(), Box<dyn Error>> {
    for value in ["0001-01-01", "0012-01-02", "2024-02-29", "9999-12-31"] {
        let day: LogDay = serde_json::from_str(&format!("\"{value}\""))?;
        assert_eq!(serde_json::to_string(&day)?, format!("\"{value}\""));
    }
    for value in [
        "0000-01-01",
        "10000-01-01",
        "2023-02-29",
        "2024-13-01",
        "2024-00-01",
        "2024-01-00",
        "2024-1-01",
        "+024-01-01",
        "-001-01-01",
        "2024/+1-01",
        "2024/01/01",
        "２０24-01-01",
        "2024-０1-01",
        "2024-01-01T00:00:00Z",
    ] {
        assert!(
            serde_json::from_str::<LogDay>(&format!("\"{value}\"")).is_err(),
            "accepted invalid log day {value}"
        );
    }
    assert!(LogDay::new(10000, 1, 1).is_err());
    assert!(LogDay::parse("2024-01-01\n").is_err());
    Ok(())
}

#[test]
fn local_time_accepts_supported_forms_and_serializes_canonically() -> Result<(), Box<dyn Error>> {
    for (input, expected) in [
        ("00:00", "00:00:00.000"),
        ("23:59", "23:59:00.000"),
        ("08:07:06", "08:07:06.000"),
        ("08:07:06.1", "08:07:06.100"),
        ("08:07:06.12", "08:07:06.120"),
        ("08:07:06.123", "08:07:06.123"),
        ("01:02:03.004", "01:02:03.004"),
    ] {
        let time = LocalTime::parse(input)?;
        assert_eq!(time.to_string(), expected);
        assert_eq!(serde_json::to_string(&time)?, format!("\"{expected}\""));
        assert_eq!(
            serde_json::from_str::<LocalTime>(&serde_json::to_string(&time)?)?,
            time
        );
    }
    for input in [
        "24:00",
        "12:60",
        "12:00:60",
        "23:59:60",
        "12:00:00.",
        "12:00.1",
        "12:00:",
        "12-00",
        "+2:00",
        "12:+0",
        "１２:00",
        "12:00:00.1x",
        "12:00:00.001Z",
        "12:00:00\n",
        "12:00:60.1",
        "12:00:00.1234",
        "12:00:00.1Z",
        "12:00+01:00",
        "12:0",
    ] {
        assert!(
            LocalTime::parse(input).is_err(),
            "accepted invalid local time {input}"
        );
    }
    assert!(LocalTime::new(23, 59, 59, 1000).is_err());
    Ok(())
}

#[test]
fn ids_payload_union_and_create_input_validate_strictly() -> Result<(), Box<dyn Error>> {
    assert_eq!(ProfileLogId::new(9)?.get(), 9);
    assert_eq!(serde_json::to_string(&ProfileLogId::new(9)?)?, "9");
    assert!(ProfileLogId::new(0).is_err());
    assert!(serde_json::from_str::<ProfileLogId>("-1").is_err());

    let expected = r#"{"type":"biometric","measurement":{"type":"weight","value":"70 kg"}}"#;
    let data: ProfileLogData = serde_json::from_str(expected)?;
    assert_eq!(serde_json::to_string(&data)?, expected);
    let input: LogInput = serde_json::from_str(
        r#"{"type":"biometric","measurement":{"type":"weight","value":"70 kg"}}"#,
    )?;
    assert!(matches!(
        input,
        LogInput::Biometric(BiometricLogEntry {
            measurement: BiometricEntry::Weight(_)
        })
    ));
    assert_eq!(
        serde_json::to_value(data)?,
        serde_json::json!({
            "type": "biometric",
            "measurement": {"type": "weight", "value": "70 kg"}
        })
    );

    for invalid in [
        r#"{"type":"biometric","measurement":{"type":"weight","value":"0 kg"}}"#,
        r#"{"type":"biometric","measurement":{"type":"weight","value":"-1 kg"}}"#,
        r#"{"type":"biometric","measurement":{"type":"weight","value":"70 kg","extra":true}}"#,
        r#"{"type":"biometric","measurement":{"type":"weight","value":"70 kg"},"current_weight":"1 kg"}"#,
        r#"{"type":"exercise"}"#,
    ] {
        assert!(
            serde_json::from_str::<ProfileLogData>(invalid).is_err(),
            "accepted invalid profile-log payload {invalid}"
        );
    }

    let create: CreateLog = serde_json::from_str(
        r#"{"day":"2024-01-01","time":null,"entry":{"type":"biometric","measurement":{"type":"weight","value":"70 kg"}}}"#,
    )?;
    assert_eq!(create.time, None);
    assert!(serde_json::from_str::<CreateLog>(
        r#"{"time":"12:00","entry":{"type":"biometric","measurement":{"type":"weight","value":"70 kg"}}}"#
    )
    .is_err());
    assert!(serde_json::from_str::<CreateLog>(
        r#"{"day":"2024-01-01","entry":{"type":"biometric","measurement":{"type":"weight","value":"70 kg"}},"profile_id":2}"#
    )
    .is_err());
    assert!(
        serde_json::from_str::<LogInput>(
            r#"{"type":"biometric","measurement":{"type":"weight","value":"70 kg"},"extra":true}"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<BiometricLogEntry>(
            r#"{"measurement":{"type":"weight","value":"70 kg"},"extra":true}"#
        )
        .is_err()
    );

    let measurement = BodyWeight::new("70 kg".parse()?)?;
    assert_eq!(serde_json::to_string(&measurement)?, "\"70 kg\"");
    Ok(())
}

#[test]
fn patch_fields_preserve_omission_and_reject_nulls() -> Result<(), Box<dyn Error>> {
    let empty: PatchLog = serde_json::from_str("{}")?;
    assert_eq!(empty, PatchLog::default());
    let patch: PatchLog = serde_json::from_str(
        r#"{"day":"2024-02-29","time":"12:13","entry":{"type":"biometric","measurement":{"type":"weight","value":"75 kg"}}}"#,
    )?;
    assert_eq!(patch.day, Patch::Set(LogDay::new(2024, 2, 29)?));
    assert_eq!(patch.time, Patch::Set(LocalTime::parse("12:13")?));
    assert!(matches!(
        patch.entry,
        Patch::Set(LogEntryPatch::Biometric(_))
    ));
    let patch_entry: LogEntryPatch = serde_json::from_str(
        r#"{"type":"biometric","measurement":{"type":"weight","value":"75 kg"}}"#,
    )?;
    assert!(matches!(
        &patch_entry,
        LogEntryPatch::Biometric(BiometricLogEntry {
            measurement: BiometricEntry::Weight(_)
        })
    ));
    let patch_data = match patch_entry {
        LogEntryPatch::Biometric(entry) => ProfileLogData::Biometric(entry),
        LogEntryPatch::Food(_) | LogEntryPatch::Journal(_) => {
            return Err(std::io::Error::other("expected biometric patch entry").into());
        }
    };
    assert_eq!(
        serde_json::to_value(patch_data)?,
        serde_json::json!({"type":"biometric","measurement":{"type":"weight","value":"75 kg"}})
    );
    for invalid in [
        r#"{"day":null}"#,
        r#"{"time":null}"#,
        r#"{"entry":null}"#,
        r#"{"entry":{"type":"biometric","measurement":{"type":"weight","value":"0 kg"}}}"#,
        r#"{"log_id":4}"#,
    ] {
        assert!(
            serde_json::from_str::<PatchLog>(invalid).is_err(),
            "accepted invalid patch {invalid}"
        );
    }
    Ok(())
}

#[test]
fn log_filters_validate_date_ranges_subtypes_and_wire_fields() -> Result<(), Box<dyn Error>> {
    let from = LogDay::new(2024, 1, 1)?;
    let to = LogDay::new(2024, 2, 1)?;
    assert!(LogFilter::new(Some(to), Some(from), None, None, None).is_err());
    assert!(LogFilter::new(None, None, None, Some(BiometricType::Weight), None).is_err());

    let filter: LogFilter = serde_json::from_str(
        r#"{"from":"2024-01-01","to":"2024-02-01","entry_type":"biometric","subtype":"weight","last_id":10}"#,
    )?;
    assert_eq!(filter.from(), Some(from));
    assert_eq!(filter.to(), Some(to));
    assert_eq!(filter.entry_type(), Some(LogEntryType::Biometric));
    assert_eq!(filter.subtype(), Some(BiometricType::Weight));
    assert_eq!(filter.last_id(), Some(ProfileLogId::new(10)?));
    assert_eq!(
        LogFilter::default(),
        LogFilter::new(None, None, None, None, None)?
    );
    assert!(
        serde_json::from_str::<LogFilter>(r#"{"from":"2024-02-01","to":"2024-01-01"}"#).is_err()
    );
    assert!(serde_json::from_str::<LogFilter>(r#"{"subtype":"weight"}"#).is_err());
    assert!(serde_json::from_str::<LogFilter>(r#"{"unknown":true}"#).is_err());
    Ok(())
}

#[test]
fn food_input_amount_units_and_serving_names_are_strict() -> Result<(), Box<dyn Error>> {
    let amount = Amount::parse("1.250")?;
    assert_eq!(amount.value(), "1.250".parse()?);
    assert_eq!(serde_json::to_string(&amount)?, r#""1.250""#);
    for invalid in ["0", "-1", "not-a-number"] {
        assert!(Amount::parse(invalid).is_err());
    }
    assert!(serde_json::from_str::<Amount>("1.5").is_err());

    for unit in ["kg", "g", "mg", "µg", "l", "ml"] {
        for selector in [unit.to_owned(), format!(" {unit} ")] {
            let parsed = ServingSelector::parse(&selector)?;
            assert!(matches!(parsed, ServingSelector::Unit(_)));
            let serialized = serde_json::to_string(&parsed)?;
            assert_eq!(
                serde_json::from_str::<ServingSelector>(&serialized)?,
                parsed
            );
            assert!(SelectedServingName::new(selector.clone()).is_err());
            assert!(
                serde_json::from_str::<SelectedServingName>(&serde_json::to_string(&selector)?)
                    .is_err()
            );
        }
    }
    let named = ServingSelector::parse("  Small bowl  ")?;
    assert!(matches!(&named, ServingSelector::Named(name) if name.as_str() == "Small bowl"));
    let named_json = serde_json::to_string(&named)?;
    assert_eq!(serde_json::from_str::<ServingSelector>(&named_json)?, named);
    assert!(ServingSelector::parse(" ").is_err());

    let input: FoodLogInput = serde_json::from_value(serde_json::json!({
        "food_id": 42,
        "amount": "2",
        "serving_size": "small bowl"
    }))?;
    assert_eq!(input.food_id().0, 42);
    assert_eq!(input.amount().value(), Decimal::from(2));
    assert_eq!(
        serde_json::to_value(input)?,
        serde_json::json!({
            "food_id": 42,
            "amount": "2",
            "serving_size": "small bowl"
        })
    );
    for invalid in [
        serde_json::json!({"food_id": 0, "amount": "1", "serving_size": "g"}),
        serde_json::json!({"food_id": 42, "amount": "0", "serving_size": "g"}),
        serde_json::json!({"food_id": 42, "amount": "1", "serving_size": " "}),
        serde_json::json!({"food_id": 42, "amount": "1", "serving_size": "g", "extra": 1}),
    ] {
        assert!(
            serde_json::from_value::<FoodLogInput>(invalid).is_err(),
            "accepted invalid food input"
        );
    }
    Ok(())
}

#[test]
fn food_snapshot_union_is_self_describing_and_validated() -> Result<(), Box<dyn Error>> {
    let input = serde_json::json!({
        "type": "food",
        "food_id": 42,
        "quantity": "50 g",
        "nutrition": {
            "calories": "100 kcal",
            "protein": "5 g"
        },
        "description": "source label",
        "input": {
            "amount": "2",
            "serving_size": {"name": "portion", "quantity": "25 g"}
        }
    });
    let data: ProfileLogData = serde_json::from_value(input)?;
    let serialized = serde_json::to_value(&data)?;
    assert_eq!(
        serialized,
        serde_json::json!({
            "type": "food",
            "food_id": 42,
            "quantity": "50 g",
            "nutrition": {
                "calories": "100 kcal",
                "fat": {
                    "total": null,
                    "saturated": null,
                    "monounsaturated": null,
                    "polyunsaturated": null,
                    "trans": null
                },
                "carbohydrates": {
                    "including_fibre": null,
                    "excluding_fibre": null,
                    "sugars": {"total": null, "added": null, "lactose": null},
                    "starch": null
                },
                "fibre": null,
                "protein": "5 g",
                "sodium": null,
                "salt": null,
                "cholesterol": null,
                "water": null,
                "caffeine": null,
                "choline": null,
                "minerals": {
                    "calcium": null, "iron": null, "magnesium": null,
                    "phosphorus": null, "potassium": null, "zinc": null,
                    "copper": null, "manganese": null, "selenium": null
                },
                "vitamins": {
                    "vitamin_a": null, "vitamin_c": null, "vitamin_d": null,
                    "vitamin_e": null, "vitamin_k1": null, "vitamin_b1": null,
                    "vitamin_b2": null, "vitamin_b3": null, "vitamin_b5": null,
                    "vitamin_b6": null, "vitamin_b9": null, "vitamin_b12": null
                }
            },
            "description": "source label",
            "input": {
                "amount": "2",
                "serving_size": {"name": "portion", "quantity": "25 g"}
            }
        })
    );
    assert!(serialized.get("entry_type").is_none());
    assert_eq!(serde_json::from_value::<ProfileLogData>(serialized)?, data);
    let ProfileLogData::Food(entry) = data else {
        return Err(std::io::Error::other("food payload decoded to another category").into());
    };
    assert_eq!(entry.food_id().map(|id| id.0), Some(42));
    assert_eq!(entry.quantity().to_string(), "50 g");
    assert_eq!(entry.nutrition().calories.to_string(), "100 kcal");
    assert_eq!(entry.description(), "source label");
    assert!(
        FoodLogEntry::new(
            entry.food_id(),
            "-1 g".parse()?,
            entry.nutrition().clone(),
            "label".to_owned(),
            None,
        )
        .is_err()
    );
    assert_eq!(
        entry
            .input()
            .map(FoodInputSnapshot::amount)
            .map(Amount::value),
        Some(Decimal::from(2))
    );

    for invalid in [
        serde_json::json!({"type":"food","food_id":0,"quantity":"50 g","nutrition":{"calories":"100 kcal"},"description":"label"}),
        serde_json::json!({"type":"food","food_id":42,"quantity":"0 g","nutrition":{"calories":"100 kcal"},"description":"label"}),
        serde_json::json!({"type":"food","food_id":42,"quantity":"-1 g","nutrition":{"calories":"100 kcal"},"description":"label"}),
        serde_json::json!({"type":"food","food_id":42,"quantity":"50 g","nutrition":{"calories":"-1 kcal"},"description":"label"}),
        serde_json::json!({"type":"food","food_id":42,"quantity":"50 g","nutrition":{"calories":"100 kcal", "protein":"-1 g"},"description":"label"}),
        serde_json::json!({"type":"food","food_id":42,"quantity":"50 g","nutrition":{"calories":"100 kcal"},"description":"label","input":{"amount":"1","serving_size":{"name":"portion","quantity":"-1 g"}}}),
        serde_json::json!({"type":"food","food_id":42,"quantity":"50 g","nutrition":{"protein":"5 g"},"description":"label"}),
        serde_json::json!({"type":"food","food_id":42,"quantity":"50 g","nutrition":{"calories":"100 kcal"},"description":"  "}),
        serde_json::json!({"type":"food","food_id":42,"quantity":"50 g","nutrition":{"calories":"100 kcal"},"description":"label","extra":1}),
        serde_json::json!({"type":"food","food_id":42,"quantity":"50 g","nutrition":{"calories":"100 kcal"},"description":"label","input":{"amount":"1","serving_size":{"name":"portion","quantity":"25 g","extra":true}}}),
    ] {
        assert!(
            serde_json::from_value::<ProfileLogData>(invalid).is_err(),
            "accepted invalid food snapshot"
        );
    }
    Ok(())
}

#[test]
fn food_log_patch_keeps_missing_fields_and_rejects_null_or_invalid_ids()
-> Result<(), Box<dyn Error>> {
    let empty: FoodLogPatch = serde_json::from_str("{}")?;
    assert!(!empty.has_changes());
    let patch: FoodLogPatch =
        serde_json::from_str(r#"{"food_id":7,"amount":"1.5","serving_size":"g"}"#)?;
    assert!(patch.has_changes());
    assert!(matches!(patch.food_id(), Patch::Set(id) if id.0 == 7));
    assert!(matches!(patch.amount(), Patch::Set(amount) if amount.value() == Decimal::new(15, 1)));
    assert!(matches!(
        patch.serving_size(),
        Patch::Set(ServingSelector::Unit(_))
    ));
    for invalid in [
        r#"{"food_id":null}"#,
        r#"{"amount":null}"#,
        r#"{"serving_size":null}"#,
        r#"{"food_id":0}"#,
        r#"{"unknown":true}"#,
        r#"{"type":"food","food_id":-1}"#,
    ] {
        assert!(
            serde_json::from_str::<FoodLogPatch>(invalid).is_err(),
            "accepted invalid food log patch {invalid}"
        );
    }
    let entry_patch: LogEntryPatch = serde_json::from_str(r#"{"type":"food"}"#)?;
    assert!(matches!(entry_patch, LogEntryPatch::Food(patch) if !patch.has_changes()));
    Ok(())
}

#[test]
fn journal_text_rejects_blank_and_preserves_original_content() -> Result<(), Box<dyn Error>> {
    for value in ["", " \n\t", "\u{2003}"] {
        assert!(JournalText::new(value).is_err());
        assert!(serde_json::from_value::<JournalText>(serde_json::json!(value)).is_err());
    }
    let value = "  Salut, lume! 🌿\nA good day.  ";
    let text = JournalText::new(value)?;
    assert_eq!(text.as_str(), value);
    assert_eq!(
        serde_json::from_value::<JournalText>(serde_json::json!(value))?,
        text
    );
    let entry: ProfileLogData =
        serde_json::from_value(serde_json::json!({"type":"journal", "text":value}))?;
    assert_eq!(
        serde_json::to_value(entry)?,
        serde_json::json!({"type":"journal", "text":value})
    );
    assert!(
        serde_json::from_value::<LogInput>(serde_json::json!({"type":"journal", "text":"  "}))
            .is_err()
    );
    assert!(
        serde_json::from_value::<LogEntryPatch>(serde_json::json!({"type":"journal", "text":"\n"}))
            .is_err()
    );
    assert!(
        LogFilter::new(
            None,
            None,
            Some(LogEntryType::Journal),
            Some(BiometricType::Weight),
            None
        )
        .is_err()
    );
    Ok(())
}
