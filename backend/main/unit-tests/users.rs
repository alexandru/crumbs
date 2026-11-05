use std::io;

use super::prompt_credentials;

#[test]
fn mismatched_passwords_fail_without_echoing_either_value() -> Result<(), io::Error> {
    let mut passwords = ["first-secret", "second-secret"].into_iter();
    let mut prompts = Vec::new();
    let result = prompt_credentials("alice".to_owned(), |message| {
        prompts.push(message.to_owned());
        passwords
            .next()
            .map(str::to_owned)
            .ok_or_else(|| io::Error::other("no test password remains"))
    });

    assert_eq!(
        prompts.first().map(String::as_str),
        Some("Password (at least 6 characters): ")
    );
    assert_eq!(
        prompts.get(1).map(String::as_str),
        Some("Confirm password: ")
    );

    let error = result
        .err()
        .ok_or_else(|| io::Error::other("mismatched passwords were accepted"))?;
    let message = error.to_string();
    assert_eq!(message, "passwords do not match");
    assert!(!message.contains("first-secret"));
    assert!(!message.contains("second-secret"));
    Ok(())
}
