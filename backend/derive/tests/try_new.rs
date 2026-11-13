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
