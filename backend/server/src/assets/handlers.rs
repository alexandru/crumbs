use axum::body::Body;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{Response, StatusCode, header::HeaderValue};
use rust_embed::Embed;

pub(super) fn stylesheet() -> Response<Body> {
    let Some(stylesheet) = StaticAssets::get("main.css") else {
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
struct StaticAssets;

#[cfg(test)]
#[path = "../../unit-tests/assets/handlers.rs"]
mod tests;
