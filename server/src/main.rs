//! Local development relay. Supply a stable 32-byte seed file and SQLite path.

use std::{env, fs, net::SocketAddr};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut args = env::args().skip(1);
    let first = args
        .next()
        .ok_or("usage: babytrack-server DB_PATH SEED_FILE BIND_ADDR | migrate-private-checkpoint DB_PATH SEED_FILE")?;
    if first == "migrate-private-checkpoint" {
        let db = args.next().ok_or("missing database path")?;
        let seed_file = args.next().ok_or("missing 32-byte seed file")?;
        if args.next().is_some() {
            return Err("extra migration arguments".into());
        }
        let seed: [u8; 32] = fs::read(seed_file)?
            .try_into()
            .map_err(|_| "seed file must contain exactly 32 bytes")?;
        babytrack_server::migrate_legacy_private_checkpoint(db, seed)?;
        eprintln!("Private integrity checkpoint established");
        return Ok(());
    }
    let db = first;
    let seed_file = args.next().ok_or("missing 32-byte seed file")?;
    let bind: SocketAddr = args.next().ok_or("missing bind address")?.parse()?;
    if args.next().is_some() {
        return Err("extra arguments".into());
    }
    let seed: [u8; 32] = fs::read(seed_file)?
        .try_into()
        .map_err(|_| "seed file must contain exactly 32 bytes")?;
    let relay_public = babytrack_wire::crypto::signing_public_key(&seed);
    let public_hex: String = relay_public
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let listener = tokio::net::TcpListener::bind(bind).await?;
    eprintln!("Relay public key: {public_hex}");
    eprintln!("Listening on {bind}");
    babytrack_server::serve(db, seed, listener).await
}
