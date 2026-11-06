use axum::body::Body;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{Response, StatusCode, header::HeaderValue};
use maud::{DOCTYPE, Markup, html};
use rust_embed::Embed;

use crate::oauth::FoodScopes;

pub(crate) struct LoginPage<'a> {
    pub request_id: &'a str,
    pub csrf_token: &'a str,
    pub client_name: &'a str,
    pub redirect_origin: &'a str,
    pub scopes: FoodScopes,
    pub error: Option<&'a str>,
}

pub(crate) fn login(page: LoginPage<'_>) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Sign in to Crumbs" }
                link rel="stylesheet" href="/assets/login.css";
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

                        p class="eyebrow" { "Account connection" }
                        h1 id="page-title" { "Sign in to Crumbs" }
                        p class="intro" { "Connect your account to continue." }

                        section class="request-summary" aria-label="Connection details" {
                            p class="summary-label" { "Requested by" }
                            p class="client-name" { (page.client_name) }
                            p class="client-origin" { (page.redirect_origin) }
                        }

                        section class="permissions" aria-labelledby="permissions-title" {
                            h2 id="permissions-title" { "This connection can" }
                            ul {
                                @if page.scopes.can_read() {
                                    li { "Read the food library" }
                                    li { "Search the food library and view nutrition data" }
                                }
                                @if page.scopes.can_write() {
                                    li { "Create, edit, and delete custom foods" }
                                }
                            }
                        }

                        form method="post" action="/oauth/login" class="login-form" {
                            input type="hidden" name="request_id" value=(page.request_id);
                            input type="hidden" name="csrf_token" value=(page.csrf_token);

                            @if let Some(error) = page.error {
                                p class="form-error" role="alert" { (error) }
                            }

                            div class="field" {
                                label for="username" { "Username" }
                                input
                                    id="username"
                                    name="username"
                                    type="text"
                                    autocomplete="username"
                                    maxlength="64"
                                    required;
                            }
                            div class="field" {
                                label for="password" { "Password" }
                                input
                                    id="password"
                                    name="password"
                                    type="password"
                                    autocomplete="current-password"
                                    required;
                            }

                            p class="safety-note" { "Only continue if you started this connection." }

                            div class="form-actions" {
                                button class="button button-primary" type="submit" name="decision" value="approve" {
                                    "Sign in & connect"
                                }
                                button class="button button-secondary" type="submit" name="decision" value="deny" formnovalidate {
                                    "Cancel"
                                }
                            }
                        }
                    }
                    footer class="page-footer" {
                        span { "Access requires your approval." }
                    }
                }
            }
        }
    }
}

pub(crate) fn stylesheet() -> Response<Body> {
    let Some(stylesheet) = LoginAssets::get("login.css") else {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::NOT_FOUND;
        return response;
    };

    let mut response = Response::new(Body::from(stylesheet.data.into_owned()));
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/css; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

#[derive(Embed)]
#[folder = "assets/"]
struct LoginAssets;

#[cfg(test)]
#[path = "../unit-tests/oauth_views.rs"]
mod tests;
