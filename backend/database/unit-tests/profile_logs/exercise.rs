use std::error::Error;

use rust_decimal::Decimal;

use crate::profile_logs::ExerciseDuration;

#[test]
fn exercise_duration_parses_exact_units_and_canonicalizes_seconds() -> Result<(), Box<dyn Error>> {
    use std::time::Duration;

    let duration = ExerciseDuration::new(Duration::from_secs(1800))?;
    assert_eq!(duration.value(), Duration::from_secs(1800));
    for value in ["1800 s", "30 min", "0.5 h", "3e1 min"] {
        let parsed: ExerciseDuration = serde_json::from_value(serde_json::json!(value))?;
        assert_eq!(parsed, duration);
        assert_eq!(serde_json::to_value(parsed)?, serde_json::json!("1800 s"));
    }
    for value in ["0.5 s", "0.000000001 s"] {
        let parsed: ExerciseDuration = serde_json::from_value(serde_json::json!(value))?;
        assert_eq!(serde_json::to_value(parsed)?, serde_json::json!(value));
    }
    let fractional_minute: ExerciseDuration =
        serde_json::from_value(serde_json::json!("0.00000000005 min"))?;
    assert_eq!(fractional_minute.value(), Duration::from_nanos(3));
    let max: ExerciseDuration =
        serde_json::from_value(serde_json::json!("18446744073709551615.999999999 s"))?;
    assert_eq!(max.value(), Duration::MAX);
    assert_eq!(
        serde_json::to_value(max)?,
        serde_json::json!("18446744073709551615.999999999 s")
    );
    for value in [
        "30min",
        "30",
        "30 ms",
        "0 min",
        "-1 min",
        "0.0000000001 s",
        "18446744073709551616 s",
        "18446744073709551616 h",
    ] {
        assert!(
            serde_json::from_value::<ExerciseDuration>(serde_json::json!(value)).is_err(),
            "accepted {value}"
        );
    }
    assert!(ExerciseDuration::new(Duration::ZERO).is_err());
    Ok(())
}

#[test]
fn exercise_type_and_calories_constructors_validate() -> Result<(), Box<dyn Error>> {
    use crate::nutrition_model::Kcal;
    use crate::profile_logs::{ExerciseCalories, ExerciseType};

    use crate::text::BlankStringError;

    let exercise_type = ExerciseType::new("  walking  ")?;
    assert_eq!(ExerciseType::new("  walking  ".to_owned())?, exercise_type);
    assert_eq!(ExerciseType::new("  "), Err(BlankStringError));
    assert_eq!(
        serde_json::to_value(&exercise_type)?,
        serde_json::json!("  walking  ")
    );
    assert_eq!(
        serde_json::from_value::<ExerciseType>(serde_json::json!("  walking  "))?,
        exercise_type
    );
    assert!(serde_json::from_value::<ExerciseType>(serde_json::json!("  ")).is_err());
    assert!(ExerciseCalories::new(Kcal::new(Decimal::NEGATIVE_ONE)).is_err());
    assert!(ExerciseCalories::new(Kcal::ZERO).is_ok());
    Ok(())
}
