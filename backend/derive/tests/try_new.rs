use crumbs_derive::TryNew;

#[derive(Debug, PartialEq, TryNew)]
struct Byte(u8);

#[derive(Debug, PartialEq, TryNew)]
struct Generic<T>(T)
where
    T: PartialEq;

#[allow(non_camel_case_types)]
#[derive(Debug, PartialEq, TryNew)]
struct Collision<__CrumbsTryNewInput>(__CrumbsTryNewInput);

#[test]
fn forwards_success_and_failure() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(Byte::new(42_u16)?, Byte(42));
    assert_eq!(Byte::new(256_u16).err(), u8::try_from(256_u16).err());
    assert_eq!(Generic::<u8>::new(42_u16)?, Generic(42));
    assert_eq!(Collision::<u8>::new(42_u16)?, Collision(42));
    Ok(())
}

mod values {
    use crumbs_derive::{Display, Value};

    #[derive(Value)]
    pub struct Text(String);

    impl Text {
        pub fn new(value: String) -> Self {
            Self(value)
        }
    }

    #[derive(Display, Value)]
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
