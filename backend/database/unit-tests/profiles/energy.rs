use std::error::Error;
use std::str::FromStr;

use rust_decimal::Decimal;

use crate::profiles::KilocaloriesPerDay;

#[test]
fn daily_energy_round_trips_exact_wire_values() -> Result<(), Box<dyn Error>> {
    let rate = KilocaloriesPerDay::from_str("1700.0000000000000000000000001 kcal")?;
    let json = serde_json::to_string(&rate)?;
    assert_eq!(json, "\"1700.0000000000000000000000001 kcal\"");
    assert_eq!(serde_json::from_str::<KilocaloriesPerDay>(&json)?, rate);
    assert_eq!(
        KilocaloriesPerDay::try_new(Decimal::ZERO)?.to_string(),
        "0 kcal"
    );
    assert!(KilocaloriesPerDay::from_str("-1 kcal").is_err());
    assert!(KilocaloriesPerDay::from_str("1 kJ").is_err());
    Ok(())
}

#[test]
fn daily_energy_addition_is_exact_and_checks_overflow() -> Result<(), Box<dyn Error>> {
    let left = KilocaloriesPerDay::from_str("1000.25 kcal")?;
    let right = KilocaloriesPerDay::from_str("50.5 kcal")?;
    assert_eq!(
        left.checked_add(right).map(|value| value.to_string()),
        Some("1050.75 kcal".to_owned())
    );

    let maximum = KilocaloriesPerDay::try_new(Decimal::MAX)?;
    let one = KilocaloriesPerDay::try_new(Decimal::ONE)?;
    assert_eq!(maximum.checked_add(one), None);
    Ok(())
}
