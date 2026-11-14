use crumbs_derive::{AliasNew, AliasTryNew};

#[derive(Debug, PartialEq, AliasTryNew)]
struct Byte(u8);

#[derive(Debug, PartialEq, AliasTryNew)]
struct Generic<T>(T)
where
    T: PartialEq;

#[allow(non_camel_case_types)]
#[derive(Debug, PartialEq, AliasTryNew)]
struct Collision<__CrumbsAliasTryNewInput>(__CrumbsAliasTryNewInput);

#[test]
fn forwards_success_and_failure() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(Byte::new(42_u16)?, Byte(42));
    assert_eq!(Byte::new(256_u16).err(), u8::try_from(256_u16).err());
    assert_eq!(Generic::<u8>::new(42_u16)?, Generic(42));
    assert_eq!(Collision::<u8>::new(42_u16)?, Collision(42));
    Ok(())
}

#[derive(Debug, PartialEq, AliasNew)]
struct Text(String);

#[derive(Debug, PartialEq, AliasNew)]
struct InfallibleGeneric<T>(T)
where
    T: PartialEq;

#[allow(non_camel_case_types)]
#[derive(Debug, PartialEq, AliasNew)]
struct InfallibleCollision<__CrumbsAliasNewInput>(__CrumbsAliasNewInput);

#[test]
fn forwards_infallible_conversions() {
    assert_eq!(Text::new("walking"), Text(String::from("walking")));
    assert_eq!(InfallibleGeneric::<u16>::new(42_u8), InfallibleGeneric(42));
    assert_eq!(
        InfallibleCollision::<u16>::new(42_u8),
        InfallibleCollision(42)
    );
}

mod values {
    use crumbs_derive::{AliasValue, Display};

    #[derive(AliasValue)]
    pub struct Text(String);

    impl Text {
        pub fn new(value: String) -> Self {
            Self(value)
        }
    }

    #[derive(Display, AliasValue)]
    pub struct Generic<T>(T)
    where
        T: PartialEq;

    impl<T: PartialEq> Generic<T> {
        pub const fn new(value: T) -> Self {
            Self(value)
        }
    }
}

const GENERIC_VALUE: values::Generic<u8> = values::Generic::new(42);
const BORROWED_VALUE: &u8 = GENERIC_VALUE.value();

#[test]
fn value_borrows_private_fields_and_supports_generics() {
    let original = String::from("walking");
    let original_ptr = original.as_ptr();
    let wrapped = values::Text::new(original);
    let borrowed: &String = wrapped.value();
    assert_eq!(borrowed, "walking");
    assert_eq!(borrowed.as_ptr(), original_ptr);
    assert_eq!(wrapped.value(), borrowed);

    let generic = values::Generic::new(String::from("moderate"));
    let generic_borrowed: &String = generic.value();
    assert_eq!(generic_borrowed, "moderate");
    assert_eq!(*BORROWED_VALUE, 42);
}

#[test]
fn display_forwards_formatting_for_generic_wrappers() {
    let value = "walk";
    let wrapped = values::Generic::new(value);
    assert_eq!(format!("{wrapped}"), value.to_string());
    assert_eq!(format!("{wrapped:>10}"), format!("{value:>10}"));
    assert_eq!(format!("{wrapped:*^10}"), format!("{value:*^10}"));
}
