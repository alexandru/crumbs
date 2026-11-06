use super::*;

#[test]
fn user_ids_require_positive_values() -> Result<(), UserError> {
    assert!(matches!(UserId::new(0), Err(UserError::InvalidUserId)));
    assert!(matches!(UserId::new(-1), Err(UserError::InvalidUserId)));
    assert_eq!(UserId::new(42)?.get(), 42);
    Ok(())
}

#[test]
fn usernames_normalize_ascii_case_and_accept_valid_punctuation() -> Result<(), UserError> {
    let username = Username::new("Alice_7.Example-2")?;
    assert_eq!(username.as_str(), "alice_7.example-2");
    assert_eq!(Username::new("ALICE_7.EXAMPLE-2")?, username);
    assert_eq!(Username::new("a".repeat(64))?.as_str().len(), 64);
    Ok(())
}

#[test]
fn usernames_reject_invalid_syntax_and_lengths() {
    for value in ["", "_alice", ".alice", "-alice", "a b", "é", "a/b"] {
        assert!(matches!(
            Username::new(value),
            Err(UserError::InvalidUsername)
        ));
    }
    assert!(matches!(
        Username::new("a".repeat(65)),
        Err(UserError::InvalidUsername)
    ));
}

#[test]
fn password_hashing_verifies_without_truncation_and_redacts_debug() -> Result<(), UserError> {
    let password = "é".repeat(6);
    let hash = PasswordHash::hash(&password)?;
    assert!(hash.verify(&password)?);
    assert!(!hash.verify("incorrect horse battery staple")?);
    assert!(!hash.verify("x")?);
    assert_eq!(PasswordHash::parse(hash.as_str())?, hash);

    let debug = format!("{hash:?}");
    assert!(!debug.contains(hash.as_str()));
    assert!(!debug.contains(&password));
    assert!(matches!(
        PasswordHash::hash("short"),
        Err(UserError::PasswordTooShort)
    ));
    assert_eq!(
        UserError::PasswordTooShort.to_string(),
        "password must be at least 6 characters"
    );
    assert!(matches!(
        PasswordHash::hash(&"é".repeat(5)),
        Err(UserError::PasswordTooShort)
    ));

    let too_long = "x".repeat(73);
    assert!(matches!(
        PasswordHash::hash(&too_long),
        Err(UserError::PasswordTooLong)
    ));
    assert_eq!(
        UserError::PasswordTooLong.to_string(),
        "password is too long"
    );
    assert!(matches!(
        hash.verify(&too_long),
        Err(UserError::PasswordTooLong)
    ));
    Ok(())
}

#[test]
fn password_hash_parser_rejects_non_bcrypt_values() {
    assert!(matches!(
        PasswordHash::parse("not-a-bcrypt-hash"),
        Err(UserError::InvalidPasswordHash)
    ));
}
