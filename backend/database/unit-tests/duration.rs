use std::error::Error;

use crate::duration::PositiveDuration;

#[test]
fn exercise_duration_parses_exact_units_and_canonicalizes_seconds() -> Result<(), Box<dyn Error>> {
    use std::time::Duration;

    let duration = PositiveDuration::try_from(Duration::from_secs(1800))?;
    assert_eq!(*duration.value(), Duration::from_secs(1800));
    for value in ["1800 s", "30 min", "0.5 h", "3e1 min"] {
        let parsed: PositiveDuration = serde_json::from_value(serde_json::json!(value))?;
        assert_eq!(parsed, duration);
        assert_eq!(serde_json::to_value(parsed)?, serde_json::json!("1800 s"));
    }
    for value in ["0.5 s", "0.000000001 s"] {
        let parsed: PositiveDuration = serde_json::from_value(serde_json::json!(value))?;
        assert_eq!(serde_json::to_value(parsed)?, serde_json::json!(value));
    }
    let fractional_minute: PositiveDuration =
        serde_json::from_value(serde_json::json!("0.00000000005 min"))?;
    assert_eq!(*fractional_minute.value(), Duration::from_nanos(3));
    let max: PositiveDuration =
        serde_json::from_value(serde_json::json!("18446744073709551615.999999999 s"))?;
    assert_eq!(*max.value(), Duration::MAX);
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
            serde_json::from_value::<PositiveDuration>(serde_json::json!(value)).is_err(),
            "accepted {value}"
        );
    }
    assert!(PositiveDuration::try_from(Duration::ZERO).is_err());
    Ok(())
}
