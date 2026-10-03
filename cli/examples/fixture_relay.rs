//! Disposable HTTP relay pinned to the published chain's time. The normal
//! server binary always uses the system clock; dynamic enrollment tests use it.

use babytrack_core::cbor::{self, Value};
use std::{env, fs, net::SocketAddr};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut args = env::args().skip(1);
    let db = args.next().ok_or("missing fixture database")?;
    let seed_file = args.next().ok_or("missing seed file")?;
    let bind: SocketAddr = args.next().ok_or("missing bind address")?.parse()?;
    if args.next().is_some() || !bind.ip().is_loopback() {
        return Err("fixture relay requires a loopback address and no extra arguments".into());
    }
    let seed: [u8; 32] = fs::read(seed_file)?
        .try_into()
        .map_err(|_| "seed must contain 32 bytes")?;
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/contiguous-chain-v1.json"))?;
    let receipt_hex =
        fixture["transitions"].as_array().unwrap().last().unwrap()["receipt_body_cbor_hex"]
            .as_str()
            .unwrap();
    let receipt: Vec<u8> = (0..receipt_hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&receipt_hex[i..i + 2], 16).unwrap())
        .collect();
    let Value::Array(receipt) = cbor::decode(&receipt)? else {
        return Err("fixture receipt must be an array".into());
    };
    let Value::Integer(now_ms) = receipt[4] else {
        return Err("fixture receipt lacks a timestamp".into());
    };
    let app = babytrack_server::test_router_at(db, seed, now_ms.try_into()?)?;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    eprintln!("Fixture relay listening on {bind}, time {now_ms}");
    axum::serve(listener, app).await?;
    Ok(())
}
