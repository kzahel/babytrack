//! Development relay routes. Only genesis promotion is enabled until the
//! remaining public control validator and read ACL are implemented.

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, OriginalUri, Path as RoutePath, State},
    http::{HeaderMap, StatusCode, Uri, header::CONTENT_TYPE},
    routing::{get, post},
};

use crate::store::RelayStore;

type Shared = Arc<Mutex<RelayStore>>;

pub(crate) fn router(
    db_path: impl AsRef<Path>,
    relay_seed: [u8; 32],
) -> Result<Router, crate::store::Error> {
    let store = Arc::new(Mutex::new(RelayStore::open(db_path, relay_seed)?));
    Ok(Router::new()
        .route(
            "/v1/families/{family}/objects/{object}",
            post(stage_genesis),
        )
        .route("/v1/families/{family}/control", post(commit_genesis))
        .route(
            "/v1/families/{family}/promotions/{promotion}",
            get(promotion_result),
        )
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .with_state(store))
}

pub async fn serve(
    db_path: impl AsRef<Path>,
    relay_seed: [u8; 32],
    listener: tokio::net::TcpListener,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = router(db_path, relay_seed).map_err(|error| format!("relay store: {error:?}"))?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn exact_post(uri: &Uri, expected: &str, headers: &HeaderMap) -> Result<(), StatusCode> {
    if uri.path() != expected || uri.query().is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if headers
        .get(CONTENT_TYPE)
        .is_none_or(|value| value.as_bytes() != b"application/cbor")
    {
        return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    Ok(())
}
fn canonical_id(text: &str) -> Result<[u8; 16], StatusCode> {
    if text.len() != 32
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut id = [0u8; 16];
    for (index, slot) in id.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
            .map_err(|_| StatusCode::BAD_REQUEST)?;
    }
    Ok(id)
}
fn cbor_response(bytes: Vec<u8>) -> ([(axum::http::HeaderName, &'static str); 1], Vec<u8>) {
    ([(CONTENT_TYPE, "application/cbor")], bytes)
}

async fn stage_genesis(
    State(store): State<Shared>,
    RoutePath((family, object)): RoutePath<(String, String)>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl axum::response::IntoResponse, StatusCode> {
    let family_id = canonical_id(&family)?;
    let object_id = canonical_id(&object)?;
    let expected = format!("/v1/families/{family}/objects/{object}");
    exact_post(&uri, &expected, &headers)?;
    let mut store = store
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let response = store
        .stage_genesis_object(family_id, object_id, &body)
        .map_err(|_| StatusCode::CONFLICT)?;
    Ok(cbor_response(response))
}

async fn commit_genesis(
    State(store): State<Shared>,
    RoutePath(family): RoutePath<String>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl axum::response::IntoResponse, StatusCode> {
    let family_id = canonical_id(&family)?;
    let expected = format!("/v1/families/{family}/control");
    exact_post(&uri, &expected, &headers)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let committed_ms: i64 = now
        .as_millis()
        .try_into()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut store = store
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let response = store
        .commit_genesis(family_id, &body, committed_ms)
        .map_err(|_| StatusCode::CONFLICT)?;
    Ok(cbor_response(response))
}

async fn promotion_result(
    State(store): State<Shared>,
    RoutePath((family, promotion)): RoutePath<(String, String)>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl axum::response::IntoResponse, StatusCode> {
    let family_id = canonical_id(&family)?;
    let promotion_id = canonical_id(&promotion)?;
    if uri.query().is_some()
        || headers
            .get(CONTENT_TYPE)
            .is_none_or(|v| v.as_bytes() != b"application/cbor")
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut store = store
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let response = store
        .promotion_result_authenticated(family_id, promotion_id, uri.path(), &body)
        .map_err(|_| StatusCode::FORBIDDEN)?;
    Ok(cbor_response(response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use babytrack_wire::cbor::Value;
    use serde_json::Value as Json;
    use tower::ServiceExt;
    fn hex(value: &str) -> Vec<u8> {
        (0..value.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
            .collect()
    }
    #[tokio::test]
    async fn genesis_http_roundtrip_matches_stage_and_authenticated_read() {
        let api: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
        )
        .unwrap();
        let chain: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
        )
        .unwrap();
        let seed: [u8; 32] = hex(chain["test_only_inputs"]["relay_sign_seed_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let app = router(dir.path().join("relay.db"), seed).unwrap();
        let wrong_family_path = api["inputs"]["stage_path"]
            .as_str()
            .unwrap()
            .replacen("123e4567", "223e4567", 1);
        let wrong_family = Request::post(wrong_family_path)
            .header(CONTENT_TYPE, "application/cbor")
            .body(Body::from(hex(api["inputs"]["stage_body_cbor_hex"]
                .as_str()
                .unwrap())))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(wrong_family).await.unwrap().status(),
            StatusCode::CONFLICT
        );
        let stage = Request::post(api["inputs"]["stage_path"].as_str().unwrap())
            .header(CONTENT_TYPE, "application/cbor")
            .body(Body::from(hex(api["inputs"]["stage_body_cbor_hex"]
                .as_str()
                .unwrap())))
            .unwrap();
        let response = app.clone().oneshot(stage).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(response.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
            hex(api["expect"]["stage_response_cbor_hex"].as_str().unwrap())
        );
        let mut bad_auth = hex(api["inputs"]["promotion_result_read_auth_cbor_hex"]
            .as_str()
            .unwrap());
        *bad_auth.last_mut().unwrap() ^= 1;
        let denied = Request::get(api["inputs"]["promotion_result_path"].as_str().unwrap())
            .header(CONTENT_TYPE, "application/cbor")
            .body(Body::from(bad_auth))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(denied).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        let result = Request::get(api["inputs"]["promotion_result_path"].as_str().unwrap())
            .header(CONTENT_TYPE, "application/cbor")
            .body(Body::from(hex(
                api["inputs"]["promotion_result_read_auth_cbor_hex"]
                    .as_str()
                    .unwrap(),
            )))
            .unwrap();
        let response = app.clone().oneshot(result).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            babytrack_wire::cbor::decode(&to_bytes(response.into_body(), 1024).await.unwrap())
                .unwrap(),
            babytrack_wire::cbor::Value::Map(vec![
                (1, babytrack_wire::cbor::Value::Integer(1)),
                (2, babytrack_wire::cbor::Value::Null)
            ])
        );
        let commit = Request::post(api["inputs"]["commit_path"].as_str().unwrap())
            .header(CONTENT_TYPE, "application/cbor")
            .body(Body::from(hex(api["inputs"]["commit_candidate_cbor_hex"]
                .as_str()
                .unwrap())))
            .unwrap();
        let response = app.clone().oneshot(commit).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let committed = to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap();
        let Value::Map(result) = babytrack_wire::cbor::decode(&committed).unwrap() else {
            panic!()
        };
        assert_eq!(result[0].1, Value::Integer(1));
        let result = Request::get(api["inputs"]["promotion_result_path"].as_str().unwrap())
            .header(CONTENT_TYPE, "application/cbor")
            .body(Body::from(hex(
                api["inputs"]["promotion_result_read_auth_cbor_hex"]
                    .as_str()
                    .unwrap(),
            )))
            .unwrap();
        let response = app.clone().oneshot(result).await.unwrap();
        assert_eq!(
            to_bytes(response.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap(),
            committed
        );
    }
}
