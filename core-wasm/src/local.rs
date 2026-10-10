use super::*;

#[wasm_bindgen]
pub struct WasmLocalFamily {
    projection: LocalProjection,
    family_id: [u8; 16],
    device_id: [u8; 16],
    last_hlc: Option<Hlc>,
}

/// Public relay authority replay. Encrypted record projection and local
#[wasm_bindgen]
impl WasmLocalFamily {
    #[wasm_bindgen(constructor)]
    pub fn new(family_id: &[u8], device_id: &[u8]) -> Result<Self, JsError> {
        let family_id = fixed(family_id, "Family ID")?;
        Ok(Self {
            projection: LocalProjection::new(family_id),
            family_id,
            device_id: fixed(device_id, "device ID")?,
            last_hlc: None,
        })
    }

    pub fn append_operation(
        &mut self,
        operation_bytes: &[u8],
        index: u64,
    ) -> Result<Vec<u8>, JsError> {
        let operation = Operation::decode_bound(operation_bytes, &self.family_id, &self.device_id)
            .map_err(debug_error)?;
        self.projection
            .append(&operation, index)
            .map_err(debug_error)?;
        self.last_hlc = Some(operation.hlc.clone());
        Ok(operation.operation_id.to_vec())
    }

    pub fn last_append_index(&self) -> u64 {
        self.projection.last_append_index()
    }

    fn identity(&self, record: [u8; 16], now_ms: i64) -> Result<web_actions::Identity, JsError> {
        let mut clock = Clock::restore(self.family_id, self.device_id, self.last_hlc.clone(), None)
            .map_err(debug_error)?;
        let stamp = clock.next(now_ms).stamp;
        Ok(web_actions::Identity {
            family: self.family_id,
            device: self.device_id,
            operation: random_v7(now_ms)?,
            record,
            stamp,
        })
    }

    pub fn create_family_operation(&self, now_ms: i64) -> Result<Vec<u8>, JsError> {
        if self.last_append_index() != 0 {
            return Err(JsError::new("Family already initialized"));
        }
        web_actions::family(self.identity(self.family_id, now_ms)?).map_err(debug_error)
    }

    pub fn create_child_operation(
        &self,
        name: &str,
        birth_day: Option<i64>,
        sex: Option<u8>,
        now_ms: i64,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::child(
            self.identity(random_v7(now_ms)?, now_ms)?,
            name,
            birth_day,
            sex,
        )
        .map_err(debug_error)
    }

    pub fn log_diaper_operation(
        &self,
        child_id: &[u8],
        kind: u8,
        now_ms: i64,
        offset_minutes: i16,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::diaper(
            self.identity(random_v7(now_ms)?, now_ms)?,
            fixed(child_id, "child ID")?,
            kind,
            now_ms,
            offset_minutes,
        )
        .map_err(debug_error)
    }

    pub fn log_bottle_operation(
        &self,
        child_id: &[u8],
        ml: u32,
        content: u8,
        now_ms: i64,
        offset_minutes: i16,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::bottle(
            self.identity(random_v7(now_ms)?, now_ms)?,
            fixed(child_id, "child ID")?,
            ml,
            content,
            now_ms,
            offset_minutes,
        )
        .map_err(debug_error)
    }

    pub fn log_note_operation(
        &self,
        child_id: &[u8],
        note: &str,
        now_ms: i64,
        offset_minutes: i16,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::note(
            self.identity(random_v7(now_ms)?, now_ms)?,
            fixed(child_id, "child ID")?,
            note,
            now_ms,
            offset_minutes,
        )
        .map_err(debug_error)
    }

    pub fn log_breast_operation(
        &self,
        child_id: &[u8],
        segments_json: &str,
        now_ms: i64,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::breast(
            self.identity(random_v7(now_ms)?, now_ms)?,
            fixed(child_id, "child ID")?,
            segments_json,
        )
        .map_err(debug_error)
    }

    pub fn edit_breast_operation(
        &self,
        child_id: &[u8],
        activity_id: &[u8],
        segments_json: &str,
        now_ms: i64,
    ) -> Result<Vec<u8>, JsError> {
        let activity_id = fixed(activity_id, "activity ID")?;
        let target = self
            .projection
            .record(&activity_id)
            .ok_or_else(|| JsError::new("breast feed target unavailable"))?;
        web_actions::edit_breast(
            self.identity(activity_id, now_ms)?,
            fixed(child_id, "child ID")?,
            target,
            segments_json,
        )
        .map_err(debug_error)
    }

    pub fn start_sleep_operation(
        &self,
        child_id: &[u8],
        now_ms: i64,
        offset_minutes: i16,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::start_sleep(
            self.identity(random_v7(now_ms)?, now_ms)?,
            fixed(child_id, "child ID")?,
            now_ms,
            offset_minutes,
        )
        .map_err(debug_error)
    }

    pub fn stop_sleep_operation(
        &self,
        child_id: &[u8],
        activity_id: &[u8],
        now_ms: i64,
        offset_minutes: i16,
    ) -> Result<Vec<u8>, JsError> {
        let activity_id = fixed(activity_id, "activity ID")?;
        let target = self
            .projection
            .record(&activity_id)
            .ok_or_else(|| JsError::new("sleep target unavailable"))?;
        web_actions::stop_sleep(
            self.identity(activity_id, now_ms)?,
            fixed(child_id, "child ID")?,
            target,
            now_ms,
            offset_minutes,
        )
        .map_err(debug_error)
    }

    pub fn snapshot_json(&self) -> String {
        web_actions::local_snapshot(&self.projection)
    }

    pub fn readable_file(&self, snapshot_utc_ms: i64) -> Result<Vec<u8>, JsError> {
        portable_file::encode_readable(
            self.family_id,
            snapshot_utc_ms,
            None,
            false,
            self.projection.records(),
        )
        .map_err(debug_error)
    }

    pub fn field_cbor(&self, record_id: &[u8], field_id: u64) -> Result<Vec<u8>, JsError> {
        let record_id = fixed(record_id, "record ID")?;
        Ok(self
            .projection
            .record(&record_id)
            .and_then(|record| record.field(field_id))
            .map_or_else(Vec::new, |field| field.canonical_bytes.clone()))
    }
}
