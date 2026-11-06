use std::error::Error as StdError;

use crate::foods::*;

#[test]
fn tag_names_fold_full_case_canonical_forms_and_latest_unicode_mappings()
-> Result<(), Box<dyn StdError>> {
    assert_eq!(TagName::new("Straße")?, TagName::new("STRASSE")?);
    assert_eq!(TagName::new("ẞ")?, TagName::new("ss")?);
    assert_eq!(TagName::new("Σ")?, TagName::new("σ")?);
    assert_eq!(TagName::new("ς")?, TagName::new("σ")?);
    assert_eq!(TagName::new("ﬃ")?, TagName::new("ffi")?);

    assert_eq!(TagName::new("é")?, TagName::new("e\u{0301}")?);
    assert_eq!(
        TagName::new("Șuncă")?,
        TagName::new("S\u{0326}UNCA\u{0306}")?
    );
    assert_ne!(TagName::new("é")?, TagName::new("e")?);
    assert_ne!(TagName::new("Ș")?, TagName::new("s")?);

    assert_eq!(TagName::new("\u{A7CE}")?, TagName::new("\u{A7CF}")?);
    assert_eq!(TagName::new("\u{16EA0}")?, TagName::new("\u{16EBB}")?);

    assert_eq!(TagName::new(" Straße ")?.as_str(), "Straße");
    Ok(())
}
