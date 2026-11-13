use super::expand_try_new;

#[test]
fn rejects_unsupported_shapes() -> Result<(), Box<dyn std::error::Error>> {
    for source in [
        "struct Empty;",
        "struct Named { value: u8 }",
        "struct Multiple(u8, u8);",
        "enum Choice { One(u8) }",
        "union Choice { value: u8 }",
    ] {
        let input = syn::parse_str(source)?;
        let error = match expand_try_new(&input) {
            Ok(_) => return Err(format!("accepted {source}").into()),
            Err(error) => error,
        };
        assert_eq!(
            error.to_string(),
            "TryNew requires a single-field tuple struct"
        );
    }
    Ok(())
}
