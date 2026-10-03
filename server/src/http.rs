//! Development relay routes for ledger-backed membership and encrypted data.

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, OriginalUri, Path as RoutePath, State},
    http::{
        HeaderMap, StatusCode, Uri,
        header::{AUTHORIZATION, CONTENT_TYPE},
    },
    routing::{get, post},
};

use crate::store::{Error as StoreError, RelayStore};
use babytrack_wire::cbor::{self, Value};

trait Clock: Send + Sync {
    fn now_ms(&self) -> Result<i64, StatusCode>;
}
struct SystemClock;
#[cfg(any(test, feature = "test-harness"))]
struct FixedClock(i64);
#[cfg(any(test, feature = "test-harness"))]
impl Clock for FixedClock {
    fn now_ms(&self) -> Result<i64, StatusCode> {
        Ok(self.0)
    }
}
impl Clock for SystemClock {
    fn now_ms(&self) -> Result<i64, StatusCode> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        now.as_millis()
            .try_into()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    }
}
struct RelayHttpState {
    store: Mutex<RelayStore>,
    clock: Arc<dyn Clock>,
}
type Shared = Arc<RelayHttpState>;

pub(crate) fn router(
    db_path: impl AsRef<Path>,
    relay_seed: [u8; 32],
) -> Result<Router, crate::store::Error> {
    router_with_clock(db_path, relay_seed, Arc::new(SystemClock))
}

#[cfg(feature = "test-harness")]
pub(crate) fn fixture_router(
    db_path: impl AsRef<Path>,
    relay_seed: [u8; 32],
    now_ms: i64,
) -> Result<Router, crate::store::Error> {
    router_with_clock(db_path, relay_seed, Arc::new(FixedClock(now_ms)))
}

fn router_with_clock(
    db_path: impl AsRef<Path>,
    relay_seed: [u8; 32],
    clock: Arc<dyn Clock>,
) -> Result<Router, crate::store::Error> {
    let store = Arc::new(RelayHttpState {
        store: Mutex::new(RelayStore::open(db_path, relay_seed)?),
        clock,
    });
    Ok(Router::new()
        .route(
            "/v1/families/{family}/objects/{object}",
            post(stage_control_object).get(read_object),
        )
        .route(
            "/v1/families/{family}/control",
            post(commit_control).get(read_control),
        )
        .route(
            "/v1/families/{family}/batches",
            post(commit_batch).get(read_batches),
        )
        .route("/v1/families/{family}/log", get(read_log))
        .route(
            "/v1/families/{family}/control-results/{transition}",
            get(read_control_result),
        )
        .route(
            "/v1/families/{family}/invites/{invitation}",
            get(read_invite),
        )
        .route(
            "/v1/families/{family}/invitation-status/{invitation}",
            get(read_invitation_status),
        )
        .route(
            "/v1/families/{family}/batch-results/{batch}",
            get(read_batch_result),
        )
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

fn read_auth(headers: &HeaderMap, body: &Bytes) -> Result<Vec<u8>, StatusCode> {
    let values: Vec<_> = headers.get_all(AUTHORIZATION).iter().collect();
    if let [value] = values.as_slice() {
        if !body.is_empty() {
            return Err(StatusCode::BAD_REQUEST);
        }
        let text = value.to_str().map_err(|_| StatusCode::BAD_REQUEST)?;
        let hex = text
            .strip_prefix("Babytrack-Read ")
            .ok_or(StatusCode::BAD_REQUEST)?;
        if hex.is_empty()
            || hex.len() > 4096
            || hex.len() % 2 != 0
            || !hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        return hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let text = std::str::from_utf8(pair).map_err(|_| StatusCode::BAD_REQUEST)?;
                u8::from_str_radix(text, 16).map_err(|_| StatusCode::BAD_REQUEST)
            })
            .collect();
    }
    if !values.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if headers
        .get(CONTENT_TYPE)
        .is_none_or(|v| v.as_bytes() != b"application/cbor")
    {
        return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    if body.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(body.to_vec())
}

fn transition_kind(body: &[u8], staged: bool) -> Result<u64, StatusCode> {
    let value = cbor::decode(body).map_err(|_| StatusCode::BAD_REQUEST)?;
    let Value::Map(fields) = value else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let index = if staged { 1 } else { 0 };
    let Some((_, Value::Map(unsigned))) = fields.get(index) else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let Some((6, Value::Integer(kind))) = unsigned.get(5) else {
        return Err(StatusCode::BAD_REQUEST);
    };
    (*kind).try_into().map_err(|_| StatusCode::BAD_REQUEST)
}

async fn stage_control_object(
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
        .store
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let kind = transition_kind(&body, true)?;
    let controls = if kind == 1 {
        0
    } else {
        store
            .current_control_count(family_id)
            .map_err(|_| StatusCode::CONFLICT)?
    };
    let response = match (kind, controls) {
        (1, _) => store.stage_genesis_object(family_id, object_id, &body),
        (2, 1) => store.stage_first_issue_object(family_id, object_id, &body),
        (11, 3) => store.stage_first_challenge_object(family_id, object_id, &body),
        (6, 5) => store.stage_first_admission_object(family_id, object_id, &body),
        (8, 6) => store.stage_first_removal_object(family_id, object_id, &body),
        (2 | 6 | 8 | 10 | 11, _) => store.stage_general_control_object(family_id, object_id, &body),
        _ => return Err(StatusCode::NOT_IMPLEMENTED),
    }
    .map_err(|_| StatusCode::CONFLICT)?;
    Ok(cbor_response(response))
}

async fn commit_control(
    State(store): State<Shared>,
    RoutePath(family): RoutePath<String>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl axum::response::IntoResponse, StatusCode> {
    let family_id = canonical_id(&family)?;
    let expected = format!("/v1/families/{family}/control");
    exact_post(&uri, &expected, &headers)?;
    let kind = transition_kind(&body, false)?;
    let clock = Arc::clone(&store.clock);
    let mut store = store
        .store
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let controls = if kind == 1 {
        0
    } else {
        store
            .current_control_count(family_id)
            .map_err(|_| StatusCode::CONFLICT)?
    };
    let response = match kind {
        4 if controls == 2 => store.commit_first_claim_with_clock(family_id, &body, || {
            clock.now_ms().map_err(|_| StoreError::Clock)
        }),
        3 | 7 | 9 => store.commit_manager_change_with_clock(family_id, &body, || {
            clock.now_ms().map_err(|_| StoreError::Clock)
        }),
        2 | 4 | 5 | 6 | 8 | 10 | 11
            if !matches!(
                (kind, controls),
                (2, 1) | (11, 3) | (5, 4) | (6, 5) | (8, 6)
            ) =>
        {
            store.commit_general_control_with_clock(family_id, &body, || {
                clock.now_ms().map_err(|_| StoreError::Clock)
            })
        }
        kind => {
            let committed_ms = clock.now_ms()?;
            match kind {
                1 => store.commit_genesis(family_id, &body, committed_ms),
                2 => store.commit_first_issue(family_id, &body, committed_ms),
                11 => store.commit_first_challenge(family_id, &body, committed_ms),
                5 => store.commit_first_proof(family_id, &body, committed_ms),
                6 => store.commit_first_admission(family_id, &body, committed_ms),
                8 => store.commit_first_removal(family_id, &body, committed_ms),
                _ => return Err(StatusCode::NOT_IMPLEMENTED),
            }
        }
    }
    .map_err(|error| match error {
        StoreError::Clock => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::CONFLICT,
    })?;
    Ok(cbor_response(response))
}

async fn commit_batch(
    State(store): State<Shared>,
    RoutePath(family): RoutePath<String>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl axum::response::IntoResponse, StatusCode> {
    let family_id = canonical_id(&family)?;
    exact_post(&uri, &format!("/v1/families/{family}/batches"), &headers)?;
    let mut store = store
        .store
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let response = store
        .commit_batch(family_id, &body)
        .map_err(|_| StatusCode::CONFLICT)?;
    Ok(cbor_response(response))
}

async fn read_log(
    State(store): State<Shared>,
    RoutePath(family): RoutePath<String>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl axum::response::IntoResponse, StatusCode> {
    let family_id = canonical_id(&family)?;
    let auth = read_auth(&headers, &body)?;
    let query = uri.query().ok_or(StatusCode::BAD_REQUEST)?;
    let digits = query
        .strip_prefix("after=")
        .ok_or(StatusCode::BAD_REQUEST)?;
    if digits.is_empty()
        || (digits.len() > 1 && digits.starts_with('0'))
        || !digits.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let after: u64 = digits.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
    let exact = uri
        .path_and_query()
        .ok_or(StatusCode::BAD_REQUEST)?
        .as_str();
    let mut store = store
        .store
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let response = store
        .log_page_authenticated(family_id, after, exact, &auth)
        .map_err(|_| StatusCode::FORBIDDEN)?;
    Ok(cbor_response(response))
}

async fn read_batches(
    State(store): State<Shared>,
    RoutePath(family): RoutePath<String>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl axum::response::IntoResponse, StatusCode> {
    let family_id = canonical_id(&family)?;
    let auth = read_auth(&headers, &body)?;
    let query = uri.query().ok_or(StatusCode::BAD_REQUEST)?;
    let digits = query
        .strip_prefix("after=")
        .ok_or(StatusCode::BAD_REQUEST)?;
    if digits.is_empty()
        || (digits.len() > 1 && digits.starts_with('0'))
        || !digits.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let after: u64 = digits.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
    let exact = uri
        .path_and_query()
        .ok_or(StatusCode::BAD_REQUEST)?
        .as_str();
    let mut store = store
        .store
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let response = store
        .batch_page_authenticated(family_id, after, exact, &auth)
        .map_err(|_| StatusCode::FORBIDDEN)?;
    Ok(cbor_response(response))
}

async fn read_control_result(
    State(store): State<Shared>,
    RoutePath((family, transition)): RoutePath<(String, String)>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl axum::response::IntoResponse, StatusCode> {
    let family_id = canonical_id(&family)?;
    let transition_id = canonical_id(&transition)?;
    if uri.query().is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let auth = read_auth(&headers, &body)?;
    let mut store = store
        .store
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let response = store
        .control_result_authenticated(family_id, transition_id, uri.path(), &auth)
        .map_err(|_| StatusCode::FORBIDDEN)?;
    Ok(cbor_response(response))
}

async fn read_invite(
    State(store): State<Shared>,
    RoutePath((family, invitation)): RoutePath<(String, String)>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl axum::response::IntoResponse, StatusCode> {
    let family_id = canonical_id(&family)?;
    let invitation_id = canonical_id(&invitation)?;
    if uri.query().is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let auth = read_auth(&headers, &body)?;
    let mut store = store
        .store
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let response = store
        .invite_authenticated(family_id, invitation_id, uri.path(), &auth)
        .map_err(|_| StatusCode::FORBIDDEN)?;
    Ok(cbor_response(response))
}

async fn read_invitation_status(
    State(store): State<Shared>,
    RoutePath((family, invitation)): RoutePath<(String, String)>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl axum::response::IntoResponse, StatusCode> {
    let family_id = canonical_id(&family)?;
    let invitation_id = canonical_id(&invitation)?;
    if uri.query().is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let auth = read_auth(&headers, &body)?;
    let now_ms = store.clock.now_ms()?;
    let mut relay = store
        .store
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let response = relay
        .invitation_status_authenticated(family_id, invitation_id, uri.path(), &auth, now_ms)
        .map_err(|_| StatusCode::FORBIDDEN)?;
    Ok(cbor_response(response))
}

async fn read_batch_result(
    State(store): State<Shared>,
    RoutePath((family, batch)): RoutePath<(String, String)>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl axum::response::IntoResponse, StatusCode> {
    let family_id = canonical_id(&family)?;
    let batch_id = canonical_id(&batch)?;
    if uri.query().is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let auth = read_auth(&headers, &body)?;
    let mut store = store
        .store
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let response = store
        .batch_result_authenticated(family_id, batch_id, uri.path(), &auth)
        .map_err(|_| StatusCode::FORBIDDEN)?;
    Ok(cbor_response(response))
}

async fn read_control(
    State(store): State<Shared>,
    RoutePath(family): RoutePath<String>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl axum::response::IntoResponse, StatusCode> {
    let family_id = canonical_id(&family)?;
    let auth = read_auth(&headers, &body)?;
    let query = uri.query().ok_or(StatusCode::BAD_REQUEST)?;
    let digits = query
        .strip_prefix("after=")
        .ok_or(StatusCode::BAD_REQUEST)?;
    if digits.is_empty()
        || (digits.len() > 1 && digits.starts_with('0'))
        || !digits.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let after: u64 = digits.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
    let exact = uri
        .path_and_query()
        .ok_or(StatusCode::BAD_REQUEST)?
        .as_str();
    let mut store = store
        .store
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let response = store
        .control_page_authenticated(family_id, after, exact, &auth)
        .map_err(|_| StatusCode::FORBIDDEN)?;
    Ok(cbor_response(response))
}

async fn read_object(
    State(store): State<Shared>,
    RoutePath((family, object)): RoutePath<(String, String)>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl axum::response::IntoResponse, StatusCode> {
    let family_id = canonical_id(&family)?;
    let object_id = canonical_id(&object)?;
    if uri.query().is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let auth = read_auth(&headers, &body)?;
    let mut store = store
        .store
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let response = store
        .object_authenticated(family_id, object_id, uri.path(), &auth)
        .map_err(|_| StatusCode::FORBIDDEN)?;
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
    if uri.query().is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let auth = read_auth(&headers, &body)?;
    let mut store = store
        .store
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let response = store
        .promotion_result_authenticated(family_id, promotion_id, uri.path(), &auth)
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

    fn chain_candidate(transition: &Json) -> (Value, Value, Vec<u8>) {
        let unsigned =
            cbor::decode(&hex(transition["unsigned_cbor_hex"].as_str().unwrap())).unwrap();
        let signatures =
            cbor::decode(&hex(transition["signatures_cbor_hex"].as_str().unwrap())).unwrap();
        let candidate = cbor::encode(&Value::Map(vec![
            (1, unsigned.clone()),
            (2, signatures.clone()),
        ]))
        .unwrap();
        (unsigned, signatures, candidate)
    }

    fn chain_time(transition: &Json) -> i64 {
        let Value::Map(fields) =
            cbor::decode(&hex(transition["committed_cbor_hex"].as_str().unwrap())).unwrap()
        else {
            panic!()
        };
        let Value::Array(receipt) = &fields[2].1 else {
            panic!()
        };
        let Value::Integer(time) = receipt[4] else {
            panic!()
        };
        time.try_into().unwrap()
    }

    fn chain_stage(transition: &Json, item: &Json, object: &[u8]) -> Vec<u8> {
        let (unsigned, signatures, _) = chain_candidate(transition);
        cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (2, unsigned),
            (3, signatures),
            (4, Value::Integer(item[0].as_u64().unwrap().into())),
            (5, Value::Bytes(hex(item[1].as_str().unwrap()))),
            (6, Value::Bytes(object.to_vec())),
        ]))
        .unwrap()
    }

    #[tokio::test]
    async fn later_repair_and_rotation_use_general_http_writer() {
        let fixture: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
        )
        .unwrap();
        let genesis: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
        )
        .unwrap();
        let seed: [u8; 32] = hex(fixture["test_only_inputs"]["relay_sign_seed_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let family: [u8; 16] = hex(genesis["inputs"]["family_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let family_hex = genesis["inputs"]["family_id_hex"].as_str().unwrap();
        let promotion: [u8; 16] = hex(genesis["inputs"]["promotion_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let transitions = fixture["transitions"].as_array().unwrap();
        let objects = fixture["objects_by_id_hex"].as_object().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("relay.db");
        let mut store = RelayStore::open(&db, seed).unwrap();
        store
            .stage_genesis_object(
                family,
                promotion,
                &hex(genesis["inputs"]["stage_body_cbor_hex"].as_str().unwrap()),
            )
            .unwrap();
        store
            .commit_genesis(
                family,
                &hex(genesis["inputs"]["commit_candidate_cbor_hex"]
                    .as_str()
                    .unwrap()),
                chain_time(&transitions[0]),
            )
            .unwrap();
        for transition in &transitions[1..=5] {
            for item in transition["manifest"].as_array().unwrap() {
                let id_hex = item[1].as_str().unwrap();
                store
                    .stage_general_control_object(
                        family,
                        hex(id_hex).try_into().unwrap(),
                        &chain_stage(transition, item, &hex(objects[id_hex].as_str().unwrap())),
                    )
                    .unwrap();
            }
            store
                .commit_general_control_with_clock(family, &chain_candidate(transition).2, || {
                    Ok(chain_time(transition))
                })
                .unwrap();
        }
        drop(store);
        for transition in &transitions[6..] {
            if transition["name"] == "remove_active" {
                let batch = hex(fixture["batch"]["envelope_cbor_hex"].as_str().unwrap());
                let app = router(&db, seed).unwrap();
                let response = app
                    .oneshot(
                        Request::post(format!("/v1/families/{family_hex}/batches"))
                            .header(CONTENT_TYPE, "application/cbor")
                            .body(Body::from(batch))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
            }
            let app =
                router_with_clock(&db, seed, Arc::new(FixedClock(chain_time(transition)))).unwrap();
            for item in transition["manifest"].as_array().unwrap() {
                let id_hex = item[1].as_str().unwrap();
                let response = app
                    .clone()
                    .oneshot(
                        Request::post(format!("/v1/families/{family_hex}/objects/{id_hex}"))
                            .header(CONTENT_TYPE, "application/cbor")
                            .body(Body::from(chain_stage(
                                transition,
                                item,
                                &hex(objects[id_hex].as_str().unwrap()),
                            )))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
            }
            let response = app
                .oneshot(
                    Request::post(format!("/v1/families/{family_hex}/control"))
                        .header(CONTENT_TYPE, "application/cbor")
                        .body(Body::from(chain_candidate(transition).2))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let Value::Map(fields) =
                cbor::decode(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap()
            else {
                panic!()
            };
            assert_eq!(
                fields[1].1,
                Value::Bytes(hex(transition["committed_cbor_hex"].as_str().unwrap()))
            );
        }
        assert!(RelayStore::open(&db, seed).is_ok());
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

    #[tokio::test]
    async fn first_issue_http_stage_commit_and_authenticated_fetch() {
        let genesis: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
        )
        .unwrap();
        let issue: Json =
            serde_json::from_str(&std::fs::read_to_string("../tests/vectors/api-v1.json").unwrap())
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
        let family: [u8; 16] = hex(genesis["inputs"]["family_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let promotion: [u8; 16] = hex(genesis["inputs"]["promotion_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let genesis_response = hex(genesis["expect"]["commit_response_cbor_hex"]
            .as_str()
            .unwrap());
        let Value::Map(fields) = cbor::decode(&genesis_response).unwrap() else {
            panic!()
        };
        let Value::Bytes(committed) = &fields[1].1 else {
            panic!()
        };
        let Value::Map(fields) = cbor::decode(committed).unwrap() else {
            panic!()
        };
        let Value::Array(receipt) = &fields[2].1 else {
            panic!()
        };
        let Value::Integer(genesis_time) = receipt[4] else {
            panic!()
        };
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("relay.db");
        let mut store = RelayStore::open(&db, seed).unwrap();
        store
            .stage_genesis_object(
                family,
                promotion,
                &hex(genesis["inputs"]["stage_body_cbor_hex"].as_str().unwrap()),
            )
            .unwrap();
        store
            .commit_genesis(
                family,
                &hex(genesis["inputs"]["commit_candidate_cbor_hex"]
                    .as_str()
                    .unwrap()),
                genesis_time.try_into().unwrap(),
            )
            .unwrap();
        drop(store);
        let app = router(&db, seed).unwrap();
        let stage = Request::post(issue["inputs"]["stage_path"].as_str().unwrap())
            .header(CONTENT_TYPE, "application/cbor")
            .body(Body::from(hex(issue["inputs"]["stage_body_cbor_hex"]
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
            hex(issue["expect"]["stage_response_cbor_hex"].as_str().unwrap())
        );
        let pending_object = Request::get(issue["inputs"]["read_object_path"].as_str().unwrap())
            .header(CONTENT_TYPE, "application/cbor")
            .body(Body::from(hex(
                issue["inputs"]["read_object_auth_cbor_hex"]
                    .as_str()
                    .unwrap(),
            )))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(pending_object).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        let commit = Request::post(issue["inputs"]["commit_path"].as_str().unwrap())
            .header(CONTENT_TYPE, "application/cbor")
            .body(Body::from(hex(issue["inputs"]["commit_body_cbor_hex"]
                .as_str()
                .unwrap())))
            .unwrap();
        let response = app.clone().oneshot(commit).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let committed = to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap();
        let Value::Map(fields) = cbor::decode(&committed).unwrap() else {
            panic!()
        };
        let Value::Bytes(issue_committed) = &fields[1].1 else {
            panic!()
        };
        let control = Request::get(issue["inputs"]["read_control_path"].as_str().unwrap())
            .header(CONTENT_TYPE, "application/cbor")
            .body(Body::from(hex(issue["inputs"]["read_auth_cbor_hex"]
                .as_str()
                .unwrap())))
            .unwrap();
        let response = app.clone().oneshot(control).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let page = to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap();
        let auth_hex = issue["inputs"]["read_auth_cbor_hex"].as_str().unwrap();
        let auth_header = issue["inputs"]["read_auth_header"].as_str().unwrap();
        assert_eq!(auth_header, format!("Babytrack-Read {auth_hex}"));
        let header_control = Request::get(issue["inputs"]["read_control_path"].as_str().unwrap())
            .header(AUTHORIZATION, auth_header)
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(header_control).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(response.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap(),
            page
        );
        let ambiguous = Request::get(issue["inputs"]["read_control_path"].as_str().unwrap())
            .header(AUTHORIZATION, auth_header)
            .body(Body::from(hex(auth_hex)))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(ambiguous).await.unwrap().status(),
            StatusCode::BAD_REQUEST
        );
        let Value::Map(page) = cbor::decode(&page).unwrap() else {
            panic!()
        };
        let Value::Array(entries) = &page[3].1 else {
            panic!()
        };
        let Value::Array(entry) = &entries[0] else {
            panic!()
        };
        assert_eq!(entry[0], Value::Integer(2));
        assert_eq!(entry[2], Value::Bytes(issue_committed.clone()));
        let object = Request::get(issue["inputs"]["read_object_path"].as_str().unwrap())
            .header(CONTENT_TYPE, "application/cbor")
            .body(Body::from(hex(
                issue["inputs"]["read_object_auth_cbor_hex"]
                    .as_str()
                    .unwrap(),
            )))
            .unwrap();
        let response = app.clone().oneshot(object).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(response.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
            hex(issue["expect"]["object_response_cbor_hex"]
                .as_str()
                .unwrap())
        );
    }

    #[tokio::test]
    async fn recipient_claim_http_commits_exact_signed_fixture() {
        let genesis: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
        )
        .unwrap();
        let issue: Json =
            serde_json::from_str(&std::fs::read_to_string("../tests/vectors/api-v1.json").unwrap())
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
        let family: [u8; 16] = hex(genesis["inputs"]["family_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let promotion: [u8; 16] = hex(genesis["inputs"]["promotion_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let issue_object: [u8; 16] = hex("083e4567e89b42d3a456426614174000").try_into().unwrap();
        let transition = &chain["transitions"][2];
        let claim_candidate = cbor::encode(&Value::Map(vec![
            (
                1,
                cbor::decode(&hex(transition["unsigned_cbor_hex"].as_str().unwrap())).unwrap(),
            ),
            (
                2,
                cbor::decode(&hex(transition["signatures_cbor_hex"].as_str().unwrap())).unwrap(),
            ),
        ]))
        .unwrap();
        let claim_committed = hex(transition["committed_cbor_hex"].as_str().unwrap());
        let Value::Map(fields) = cbor::decode(&claim_committed).unwrap() else {
            panic!()
        };
        let Value::Array(receipt) = &fields[2].1 else {
            panic!()
        };
        let Value::Integer(claim_time) = receipt[4] else {
            panic!()
        };
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("relay.db");
        let mut store = RelayStore::open(&db, seed).unwrap();
        store
            .stage_genesis_object(
                family,
                promotion,
                &hex(genesis["inputs"]["stage_body_cbor_hex"].as_str().unwrap()),
            )
            .unwrap();
        let genesis_committed = hex(chain["transitions"][0]["committed_cbor_hex"]
            .as_str()
            .unwrap());
        let Value::Map(fields) = cbor::decode(&genesis_committed).unwrap() else {
            panic!()
        };
        let Value::Array(receipt) = &fields[2].1 else {
            panic!()
        };
        let Value::Integer(genesis_time) = receipt[4] else {
            panic!()
        };
        store
            .commit_genesis(
                family,
                &hex(genesis["inputs"]["commit_candidate_cbor_hex"]
                    .as_str()
                    .unwrap()),
                genesis_time.try_into().unwrap(),
            )
            .unwrap();
        store
            .stage_first_issue_object(
                family,
                issue_object,
                &hex(issue["inputs"]["stage_body_cbor_hex"].as_str().unwrap()),
            )
            .unwrap();
        let issue_committed = hex(chain["transitions"][1]["committed_cbor_hex"]
            .as_str()
            .unwrap());
        let Value::Map(fields) = cbor::decode(&issue_committed).unwrap() else {
            panic!()
        };
        let Value::Array(receipt) = &fields[2].1 else {
            panic!()
        };
        let Value::Integer(issue_time) = receipt[4] else {
            panic!()
        };
        store
            .commit_first_issue(
                family,
                &hex(issue["inputs"]["commit_body_cbor_hex"].as_str().unwrap()),
                issue_time.try_into().unwrap(),
            )
            .unwrap();
        drop(store);
        let app = router_with_clock(
            &db,
            seed,
            Arc::new(FixedClock(claim_time.try_into().unwrap())),
        )
        .unwrap();
        let claim_path = issue["inputs"]["commit_path"].as_str().unwrap();
        for _ in 0..2 {
            let request = Request::post(claim_path)
                .header(CONTENT_TYPE, "application/cbor")
                .body(Body::from(claim_candidate.clone()))
                .unwrap();
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                to_bytes(response.into_body(), 2 * 1024 * 1024)
                    .await
                    .unwrap()
                    .to_vec(),
                crate::receipt::control_commit_response(&claim_committed).unwrap()
            );
        }
        drop(app);
        let challenge = &chain["transitions"][3];
        let challenge_unsigned =
            cbor::decode(&hex(challenge["unsigned_cbor_hex"].as_str().unwrap())).unwrap();
        let challenge_signatures =
            cbor::decode(&hex(challenge["signatures_cbor_hex"].as_str().unwrap())).unwrap();
        let challenge_candidate = cbor::encode(&Value::Map(vec![
            (1, challenge_unsigned.clone()),
            (2, challenge_signatures.clone()),
        ]))
        .unwrap();
        let challenge_committed = hex(challenge["committed_cbor_hex"].as_str().unwrap());
        let Value::Map(fields) = cbor::decode(&challenge_committed).unwrap() else {
            panic!()
        };
        let Value::Array(receipt) = &fields[2].1 else {
            panic!()
        };
        let Value::Integer(challenge_time) = receipt[4] else {
            panic!()
        };
        let app = router_with_clock(
            &db,
            seed,
            Arc::new(FixedClock(challenge_time.try_into().unwrap())),
        )
        .unwrap();
        for entry in challenge["manifest"].as_array().unwrap() {
            let kind = entry[0].as_u64().unwrap();
            let id_text = entry[1].as_str().unwrap();
            let id = hex(id_text);
            let object = hex(chain["objects_by_id_hex"][id_text].as_str().unwrap());
            let body = cbor::encode(&Value::Map(vec![
                (1, Value::Integer(1)),
                (2, challenge_unsigned.clone()),
                (3, challenge_signatures.clone()),
                (4, Value::Integer(kind.into())),
                (5, Value::Bytes(id)),
                (6, Value::Bytes(object)),
            ]))
            .unwrap();
            let path = format!(
                "/v1/families/{}/objects/{}",
                genesis["inputs"]["family_id_hex"].as_str().unwrap(),
                id_text
            );
            let request = Request::post(path)
                .header(CONTENT_TYPE, "application/cbor")
                .body(Body::from(body))
                .unwrap();
            assert_eq!(
                app.clone().oneshot(request).await.unwrap().status(),
                StatusCode::OK
            );
        }
        let request = Request::post(claim_path)
            .header(CONTENT_TYPE, "application/cbor")
            .body(Body::from(challenge_candidate))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(response.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
            crate::receipt::control_commit_response(&challenge_committed).unwrap()
        );
        drop(app);
        let proof = &chain["transitions"][4];
        let proof_candidate = cbor::encode(&Value::Map(vec![
            (
                1,
                cbor::decode(&hex(proof["unsigned_cbor_hex"].as_str().unwrap())).unwrap(),
            ),
            (
                2,
                cbor::decode(&hex(proof["signatures_cbor_hex"].as_str().unwrap())).unwrap(),
            ),
        ]))
        .unwrap();
        let proof_committed = hex(proof["committed_cbor_hex"].as_str().unwrap());
        let Value::Map(fields) = cbor::decode(&proof_committed).unwrap() else {
            panic!()
        };
        let Value::Array(receipt) = &fields[2].1 else {
            panic!()
        };
        let Value::Integer(proof_time) = receipt[4] else {
            panic!()
        };
        let app = router_with_clock(
            &db,
            seed,
            Arc::new(FixedClock(proof_time.try_into().unwrap())),
        )
        .unwrap();
        let request = Request::post(claim_path)
            .header(CONTENT_TYPE, "application/cbor")
            .body(Body::from(proof_candidate))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(response.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
            crate::receipt::control_commit_response(&proof_committed).unwrap()
        );
        let admission = &chain["transitions"][5];
        let admission_unsigned =
            cbor::decode(&hex(admission["unsigned_cbor_hex"].as_str().unwrap())).unwrap();
        let admission_signatures =
            cbor::decode(&hex(admission["signatures_cbor_hex"].as_str().unwrap())).unwrap();
        let admission_candidate = cbor::encode(&Value::Map(vec![
            (1, admission_unsigned.clone()),
            (2, admission_signatures.clone()),
        ]))
        .unwrap();
        let admission_committed = hex(admission["committed_cbor_hex"].as_str().unwrap());
        let Value::Map(fields) = cbor::decode(&admission_committed).unwrap() else {
            panic!()
        };
        let Value::Array(receipt) = &fields[2].1 else {
            panic!()
        };
        let Value::Integer(admission_time) = receipt[4] else {
            panic!()
        };
        let app = router_with_clock(
            &db,
            seed,
            Arc::new(FixedClock(admission_time.try_into().unwrap())),
        )
        .unwrap();
        for entry in admission["manifest"].as_array().unwrap() {
            let kind = entry[0].as_u64().unwrap();
            let id_text = entry[1].as_str().unwrap();
            let id = hex(id_text);
            let object = hex(chain["objects_by_id_hex"][id_text].as_str().unwrap());
            let body = cbor::encode(&Value::Map(vec![
                (1, Value::Integer(1)),
                (2, admission_unsigned.clone()),
                (3, admission_signatures.clone()),
                (4, Value::Integer(kind.into())),
                (5, Value::Bytes(id)),
                (6, Value::Bytes(object)),
            ]))
            .unwrap();
            let path = format!(
                "/v1/families/{}/objects/{}",
                genesis["inputs"]["family_id_hex"].as_str().unwrap(),
                id_text
            );
            let request = Request::post(path)
                .header(CONTENT_TYPE, "application/cbor")
                .body(Body::from(body))
                .unwrap();
            assert_eq!(
                app.clone().oneshot(request).await.unwrap().status(),
                StatusCode::OK
            );
        }
        let request = Request::post(claim_path)
            .header(CONTENT_TYPE, "application/cbor")
            .body(Body::from(admission_candidate))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(response.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
            crate::receipt::control_commit_response(&admission_committed).unwrap()
        );
    }
}
