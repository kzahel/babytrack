use super::*;

#[wasm_bindgen]
pub struct WasmRemovalProbe {
    pub(super) inner: removal_probe::RemovalControlProbe,
}

#[wasm_bindgen]
impl WasmRemovalProbe {
    pub fn cursor(&self) -> u64 {
        self.inner.cursor()
    }

    pub fn accept_page(&mut self, page_bytes: &[u8], after: u64) -> Result<String, JsError> {
        let (proof, has_more) = self.inner.accept(page_bytes, after).map_err(debug_error)?;
        let removal = if let Some(proof) = proof {
            let transition: String = proof
                .transition_id
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            format!(
                "{{\"transitionId\":\"{transition}\",\"cursor\":{},\"sourceCursor\":{},\"knownGap\":{}}}",
                proof.cursor, proof.source_cursor, proof.known_gap,
            )
        } else {
            "null".to_owned()
        };
        Ok(format!(
            "{{\"cursor\":{},\"hasMore\":{has_more},\"removal\":{removal}}}",
            self.inner.cursor(),
        ))
    }
}
