//! RFC 9562 UUID version and variant checks for protocol identities.

pub(crate) fn is_v4(id: &[u8; 16]) -> bool {
    id[6] >> 4 == 4 && id[8] >> 6 == 2
}

pub(crate) fn is_v7(id: &[u8; 16]) -> bool {
    id[6] >> 4 == 7 && id[8] >> 6 == 2
}
