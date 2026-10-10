use super::*;

#[wasm_bindgen]
pub struct WasmReadableRestore {
    operations: Vec<Vec<u8>>,
}

#[wasm_bindgen]
impl WasmReadableRestore {
    #[wasm_bindgen(constructor)]
    pub fn new(
        readable: &[u8],
        family_id: &[u8],
        device_id: &[u8],
        now_ms: i64,
    ) -> Result<Self, JsError> {
        let parsed = portable_file::parse_readable(readable).map_err(debug_error)?;
        let family_id = fixed(family_id, "new Family ID")?;
        let device_id = fixed(device_id, "new device ID")?;
        let mut seen = BTreeSet::new();
        let count = portable_file::restore_operation_count(&parsed);
        let mut ids = Vec::with_capacity(count);
        for _ in 0..count {
            let mut id = random_v7(now_ms)?;
            while !seen.insert(id) {
                id = random_v7(now_ms)?;
            }
            ids.push(id);
        }
        let operations =
            portable_file::restore_operations(&parsed, family_id, device_id, now_ms, &ids)
                .map_err(debug_error)?
                .iter()
                .map(Operation::encode_new)
                .collect::<Result<Vec<_>, _>>()
                .map_err(debug_error)?;
        Ok(Self { operations })
    }

    pub fn count(&self) -> usize {
        self.operations.len()
    }

    pub fn operation(&self, index: usize) -> Result<Vec<u8>, JsError> {
        self.operations
            .get(index)
            .cloned()
            .ok_or_else(|| JsError::new("restore operation index outside range"))
    }
}

/// Encrypt a readable backup under the shared Argon2id file contract. The
/// caller passes a memory estimate; a low estimate fails rather than weakening it.
#[wasm_bindgen]
pub fn protect_backup(
    readable: &[u8],
    password: &str,
    available_memory_bytes: u64,
) -> Result<Vec<u8>, JsError> {
    portable_file::protect_readable(readable, password, available_memory_bytes).map_err(debug_error)
}

/// Decrypt a protected backup to its readable bytes; a wrong password and a
/// damaged file fail the same way.
#[wasm_bindgen]
pub fn open_protected_backup(
    protected: &[u8],
    password: &str,
    available_memory_bytes: u64,
) -> Result<Vec<u8>, JsError> {
    portable_file::open_protected(protected, password, available_memory_bytes)
        .map_err(|_| JsError::new("protected-failure"))
}

/// What a readable backup contains, for the restore preview.
#[wasm_bindgen]
pub fn inspect_backup(readable: &[u8]) -> Result<String, JsError> {
    let parsed = portable_file::parse_readable(readable).map_err(debug_error)?;
    Ok(format!(
        "{{\"snapshotMs\":{},\"records\":{},\"knownGap\":{}}}",
        parsed.snapshot_utc_ms,
        parsed.rows.len(),
        parsed.known_gap
    ))
}
