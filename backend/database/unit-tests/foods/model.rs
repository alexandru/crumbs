use std::error::Error as StdError;

use super::FoodLimit;

#[test]
fn search_limits_default_and_clamp_to_the_contract_range() -> Result<(), Box<dyn StdError>> {
    assert_eq!(FoodLimit::default().get(), 20);
    for (input, expected) in [
        (i64::MIN, 20),
        (-1, 20),
        (0, 20),
        (1, 20),
        (19, 20),
        (20, 20),
        (1_000, 1_000),
        (1_001, 1_000),
        (i64::MAX, 1_000),
    ] {
        assert_eq!(FoodLimit::from(input).get(), expected, "input {input}");
    }
    for (json, expected) in [("1", 20), ("20", 20), ("1001", 1_000)] {
        let decoded: FoodLimit = serde_json::from_str(json)?;
        assert_eq!(decoded.get(), expected, "input {json}");
    }
    Ok(())
}
