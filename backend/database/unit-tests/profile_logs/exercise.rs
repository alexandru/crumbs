use std::error::Error;

use rust_decimal::Decimal;

use crate::profile_logs::ExerciseDuration;

#[test]
fn exercise_duration_parses_and_formats() -> Result<(), Box<dyn Error>> {
    use std::time::Duration;

    let duration = ExerciseDuration::new(Duration::from_secs(1800));
    assert_eq!(*duration.value(), Duration::from_secs(1800));
    for value in ["1800 s", "30 min", "0.5 h", "30min", "20 min 10 min"] {
        let parsed: ExerciseDuration = serde_json::from_value(serde_json::json!(value))?;
        assert_eq!(parsed, duration);
        assert_eq!(serde_json::to_value(parsed)?, serde_json::json!("1800 s"));
        assert_eq!(parsed.to_string(), "1800 s");
    }
    for (value, expected) in [
        ("0 min", "0 s"),
        ("0.5 s", "0.5 s"),
        ("0.000000001 s", "0.000000001 s"),
    ] {
        let parsed: ExerciseDuration = serde_json::from_value(serde_json::json!(value))?;
        assert_eq!(serde_json::to_value(parsed)?, serde_json::json!(expected));
    }
    let max = ExerciseDuration::new(Duration::MAX);
    let encoded = serde_json::to_value(max)?;
    assert_eq!(
        encoded,
        serde_json::json!("18446744073709551615.999999999 s")
    );
    assert_eq!(serde_json::from_value::<ExerciseDuration>(encoded)?, max);
    assert_eq!(
        *ExerciseDuration::new(Duration::ZERO).value(),
        Duration::ZERO
    );
    for value in ["-1 min", "invalid", "18446744073709551616 s"] {
        assert!(
            serde_json::from_value::<ExerciseDuration>(serde_json::json!(value)).is_err(),
            "accepted {value}"
        );
    }
    Ok(())
}

#[test]
fn exercise_type_and_calories_constructors_validate() -> Result<(), Box<dyn Error>> {
    use crate::nutrition_model::{Kcal, QuantityError};
    use crate::profile_logs::{ExerciseCalories, ExerciseType};

    use crate::text::BlankStringError;

    let exercise_type = ExerciseType::new("  walking  ")?;
    assert_eq!(ExerciseType::new("  walking  ".to_owned())?, exercise_type);
    assert_eq!(exercise_type.value().as_str(), "  walking  ");
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
    assert_eq!(
        ExerciseCalories::new(Kcal::new(Decimal::NEGATIVE_ONE)),
        Err(QuantityError::Negative)
    );
    assert_eq!(
        *ExerciseCalories::new(Kcal::ZERO)?.value().value(),
        Kcal::ZERO
    );
    assert_eq!(
        *ExerciseCalories::new(Kcal::from(1))?.value().value(),
        Kcal::from(1)
    );
    Ok(())
}
