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
