use std::error::Error;

use rust_decimal::Decimal;

use crate::profile_logs::ExerciseDuration;

#[test]
fn exercise_duration_uses_shared_measurement_rules() -> Result<(), Box<dyn Error>> {
    use crate::nutrition_model::Minutes;

    let minutes = Minutes::new(Decimal::from(30));
    let duration = ExerciseDuration::new(minutes)?;
    assert_eq!(duration.value(), minutes);
    assert_eq!(serde_json::to_value(duration)?, serde_json::json!("30 min"));
    for value in ["30.5 min", "3.05e1 min"] {
        let duration: ExerciseDuration = serde_json::from_value(serde_json::json!(value))?;
        assert_eq!(duration.value().value(), Decimal::new(305, 1));
    }
    for value in [
        "30min",
        "30",
        "30 s",
        "0 min",
        "-1 min",
        "0.00000000000000000000000000001 min",
    ] {
        assert!(
            serde_json::from_value::<ExerciseDuration>(serde_json::json!(value)).is_err(),
            "accepted {value}"
        );
    }
    assert!(ExerciseDuration::new(Minutes::new(Decimal::ZERO)).is_err());
    assert!(ExerciseDuration::new(Minutes::new(Decimal::NEGATIVE_ONE)).is_err());
    Ok(())
}

#[test]
fn exercise_type_and_calories_constructors_validate() {
    use crate::nutrition_model::Kcal;
    use crate::profile_logs::{ExerciseCalories, ExerciseType};

    assert!(ExerciseType::new("  ").is_err());
    assert!(ExerciseCalories::new(Kcal::new(Decimal::NEGATIVE_ONE)).is_err());
    assert!(ExerciseCalories::new(Kcal::ZERO).is_ok());
}
