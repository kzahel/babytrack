//! Explicit cross-origin access for a separately hosted web client.
//!
//! Relay requests carry their own device signatures, never cookies, so
//! allowing a listed web origin grants it no ambient authority. Only exact
//! origins named at startup are answered; any other origin gets the relay's
//! ordinary same-origin behavior.

use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    extract::Request,
    http::{
        HeaderValue, Method, StatusCode,
        header::{
            ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS,
            ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_MAX_AGE, ORIGIN, VARY,
        },
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
};

/// Parse a comma-separated list of web origins such as
/// `https://lantern.example.com`. Each must be a bare scheme and host, with
/// an optional port, and no path, query, or wildcard.
pub fn parse_allowed_origins(list: &str) -> Result<Vec<String>, String> {
    list.split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .map(|origin| {
            let rest = origin
                .strip_prefix("https://")
                .or_else(|| origin.strip_prefix("http://"))
                .ok_or_else(|| format!("allowed origin must be http(s): {origin}"))?;
            let valid = !rest.is_empty()
                && rest
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'));
            if valid {
                Ok(origin.to_owned())
            } else {
                Err(format!(
                    "allowed origin must be a bare scheme and host: {origin}"
                ))
            }
        })
        .collect()
}

pub(crate) fn allow_origins(router: Router, origins: Vec<String>) -> Router {
    if origins.is_empty() {
        return router;
    }
    let allowed: Arc<[String]> = origins.into();
    router.layer(middleware::from_fn(move |request: Request, next: Next| {
        let allowed = Arc::clone(&allowed);
        async move { answer(&allowed, request, next).await }
    }))
}

async fn answer(allowed: &[String], request: Request, next: Next) -> Response {
    let origin = request
        .headers()
        .get(ORIGIN)
        .filter(|value| {
            allowed
                .iter()
                .any(|origin| value.as_bytes() == origin.as_bytes())
        })
        .cloned();
    let Some(origin) = origin else {
        return next.run(request).await;
    };
    let preflight = request.method() == Method::OPTIONS;
    let mut response = if preflight {
        (StatusCode::NO_CONTENT, Body::empty()).into_response()
    } else {
        next.run(request).await
    };
    let headers = response.headers_mut();
    headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    headers.append(VARY, HeaderValue::from_static("Origin"));
    if preflight {
        headers.insert(
            ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, POST"),
        );
        headers.insert(
            ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("authorization, content-type"),
        );
        headers.insert(ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("600"));
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use tower::ServiceExt;

    fn app() -> Router {
        allow_origins(
            Router::new().route("/v1/ping", get(|| async { "ok" })),
            vec!["https://lantern.example.com".to_owned()],
        )
    }

    fn request(method: Method, origin: Option<&str>) -> Request {
        let mut builder = Request::builder().method(method).uri("/v1/ping");
        if let Some(origin) = origin {
            builder = builder.header(ORIGIN, origin);
        }
        builder.body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn listed_origin_gets_preflight_and_response_headers() {
        let preflight = app()
            .oneshot(request(
                Method::OPTIONS,
                Some("https://lantern.example.com"),
            ))
            .await
            .unwrap();
        assert_eq!(preflight.status(), StatusCode::NO_CONTENT);
        let headers = preflight.headers();
        assert_eq!(
            headers[ACCESS_CONTROL_ALLOW_ORIGIN],
            "https://lantern.example.com"
        );
        assert_eq!(headers[ACCESS_CONTROL_ALLOW_METHODS], "GET, POST");
        assert_eq!(
            headers[ACCESS_CONTROL_ALLOW_HEADERS],
            "authorization, content-type"
        );
        let read = app()
            .oneshot(request(Method::GET, Some("https://lantern.example.com")))
            .await
            .unwrap();
        assert_eq!(read.status(), StatusCode::OK);
        assert_eq!(
            read.headers()[ACCESS_CONTROL_ALLOW_ORIGIN],
            "https://lantern.example.com"
        );
    }

    #[tokio::test]
    async fn other_origins_and_same_origin_requests_are_unchanged() {
        for origin in [
            Some("https://evil.example.com"),
            Some("https://lantern.example.com.evil"),
            None,
        ] {
            let preflight = app()
                .oneshot(request(Method::OPTIONS, origin))
                .await
                .unwrap();
            assert_eq!(preflight.status(), StatusCode::METHOD_NOT_ALLOWED);
            assert!(
                preflight
                    .headers()
                    .get(ACCESS_CONTROL_ALLOW_ORIGIN)
                    .is_none()
            );
            let read = app().oneshot(request(Method::GET, origin)).await.unwrap();
            assert_eq!(read.status(), StatusCode::OK);
            assert!(read.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
        }
    }

    #[test]
    fn origin_list_rejects_paths_and_wildcards() {
        assert_eq!(
            parse_allowed_origins(" https://a.example.com , http://localhost:5173 ").unwrap(),
            vec!["https://a.example.com", "http://localhost:5173"]
        );
        for bad in [
            "https://*.example.com",
            "https://a.example.com/",
            "ftp://a.example.com",
            "a.example.com",
        ] {
            assert!(parse_allowed_origins(bad).is_err(), "{bad}");
        }
        assert!(parse_allowed_origins("").unwrap().is_empty());
    }
}
