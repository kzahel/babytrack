//! RFC 9562 UUID version and variant checks for protocol identities.

pub(crate) fn is_v4(id: &[u8; 16]) -> bool {
    id[6] >> 4 == 4 && id[8] >> 6 == 2
}

pub(crate) fn is_v7(id: &[u8; 16]) -> bool {
    id[6] >> 4 == 7 && id[8] >> 6 == 2
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn random_v4() -> Result<[u8; 16], getrandom::Error> {
    let mut id = [0u8; 16];
    getrandom::fill(&mut id)?;
    id[6] = (id[6] & 0x0f) | 0x40;
    id[8] = (id[8] & 0x3f) | 0x80;
    Ok(id)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn random_v7(now_ms: i64) -> Result<[u8; 16], getrandom::Error> {
    let mut id = [0u8; 16];
    getrandom::fill(&mut id)?;
    let timestamp = (now_ms as u64).to_be_bytes();
    id[..6].copy_from_slice(&timestamp[2..]);
    id[6] = (id[6] & 0x0f) | 0x70;
    id[8] = (id[8] & 0x3f) | 0x80;
    Ok(id)
}
