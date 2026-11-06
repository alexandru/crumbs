use maud::{DOCTYPE, Markup, html};

pub(super) async fn hello() -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Crumbs" }
                link rel="stylesheet" href="/assets/main.css";
            }
            body {
                main class="page-shell" {
                    section class="login-card" aria-labelledby="page-title" {
                        header class="brand" aria-label="Crumbs" {
                            span class="brand-mark" aria-hidden="true" {
                                span class="crumb crumb-one" {}
                                span class="crumb crumb-two" {}
                                span class="crumb crumb-three" {}
                            }
                            span class="brand-name" { "crumbs" }
                        }

                        p class="eyebrow" { "Food library" }
                        h1 id="page-title" { "Hello, welcome to Crumbs." }
                        p class="intro" { "A little clarity about what you eat." }
                        p class="safety-note" { "Your food library is ready." }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../../unit-tests/home/handlers.rs"]
mod tests;
