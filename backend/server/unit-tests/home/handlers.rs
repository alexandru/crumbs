use super::*;

#[tokio::test]
async fn root_handler_renders_welcome_page() {
    let markup = hello().await.into_string();

    for text in [
        "Hello, welcome to Crumbs.",
        "A little clarity about what you eat.",
        "Your food library is ready.",
    ] {
        assert!(markup.contains(text), "missing welcome text {text:?}");
    }
    assert!(markup.contains("class=\"crumb crumb-three\""));
    assert!(!markup.contains("<form"));
    assert!(!markup.contains("<a "));
}
