//! Local development relay. Supply a stable 32-byte seed file and SQLite path.

use std::{env, fs, net::SocketAddr};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut args = env::args().skip(1);
    let db = args
        .next()
        .ok_or("usage: babytrack-server DB_PATH SEED_FILE BIND_ADDR")?;
    let seed_file = args.next().ok_or("missing 32-byte seed file")?;
    let bind: SocketAddr = args.next().ok_or("missing bind address")?.parse()?;
    if args.next().is_some() {
        return Err("extra arguments".into());
    }
    let seed: [u8; 32] = fs::read(seed_file)?
        .try_into()
        .map_err(|_| "seed file must contain exactly 32 bytes")?;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    babytrack_server::serve(db, seed, listener).await
}
