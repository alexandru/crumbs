use std::error::Error;

use rust_decimal::Decimal;

use crate::profile_logs::ExerciseDuration;

#[test]
fn exercise_duration_wraps_validated_duration() -> Result<(), Box<dyn Error>> {
    use std::time::Duration;

    use crate::duration::DurationError;

    let duration = ExerciseDuration::new(Duration::from_secs(1800))?;
    assert_eq!(*duration.value().value(), Duration::from_secs(1800));
    assert_eq!(serde_json::to_value(duration)?, serde_json::json!("1800 s"));
    assert_eq!(
        ExerciseDuration::new(Duration::ZERO),
        Err(DurationError::NonPositive)
    );
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
    Ok(())
}
