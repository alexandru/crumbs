use std::error::Error;

use crate::text::NonBlankString;

#[test]
fn rejects_blank_text_and_preserves_supplied_content() -> Result<(), Box<dyn Error>> {
    for value in ["", " \n\t", "\u{2003}"] {
        assert!(NonBlankString::new(value).is_err());
        assert!(serde_json::from_value::<NonBlankString>(serde_json::json!(value)).is_err());
    }

    let value = "  Café\n水\0  ";
    let text = NonBlankString::new(value)?;
    assert_eq!(text.as_str(), value);
    assert_eq!(serde_json::to_value(&text)?, serde_json::json!(value));
    assert_eq!(
        serde_json::from_value::<NonBlankString>(serde_json::json!(value))?,
        text
    );
    assert_eq!(text.into_inner(), value);
    Ok(())
}
