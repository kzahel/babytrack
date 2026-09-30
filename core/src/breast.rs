//! Shared validation for a completed breast feed's atomic segment field.

use crate::cbor::Value;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct Segment {
    pub side: u8,
    pub start_utc_ms: i64,
    pub end_utc_ms: i64,
    pub start_offset_minutes: i16,
    pub end_offset_minutes: i16,
}

pub fn fields(
    segments: &[Segment],
    start_utc_ms: i64,
    start_offset_minutes: i16,
    saved_at_ms: i64,
) -> Result<Vec<(u64, Value)>, &'static str> {
    if segments.is_empty()
        || segments.len() > 8
        || segments[0].start_utc_ms != start_utc_ms
        || segments[0].start_offset_minutes != start_offset_minutes
    {
        return Err("breast segment count or start invalid");
    }
    let mut previous_end = start_utc_ms;
    let mut active_ms = 0_i64;
    let mut encoded = Vec::with_capacity(segments.len());
    for segment in segments {
        if !(1..=2).contains(&segment.side)
            || !(-840..=840).contains(&segment.start_offset_minutes)
            || !(-840..=840).contains(&segment.end_offset_minutes)
            || segment.start_utc_ms < previous_end
            || segment.end_utc_ms <= segment.start_utc_ms
            || segment.end_utc_ms > saved_at_ms
        {
            return Err("breast segment side or interval invalid");
        }
        active_ms = active_ms
            .checked_add(segment.end_utc_ms - segment.start_utc_ms)
            .ok_or("breast feed duration invalid")?;
        let start = Value::Array(vec![
            Value::Integer(segment.start_utc_ms.into()),
            Value::Integer(segment.start_offset_minutes.into()),
        ]);
        let end = Value::Array(vec![
            Value::Integer(segment.end_utc_ms.into()),
            Value::Integer(segment.end_offset_minutes.into()),
        ]);
        encoded.push(Value::Array(vec![
            Value::Integer(segment.side.into()),
            start,
            end,
        ]));
        previous_end = segment.end_utc_ms;
    }
    if active_ms > 240 * 60_000 {
        return Err("breast feed duration exceeds four hours");
    }
    let end = Value::Array(vec![
        Value::Integer(previous_end.into()),
        Value::Integer(segments.last().unwrap().end_offset_minutes.into()),
    ]);
    Ok(vec![(2, end), (100, Value::Array(encoded))])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_gap_is_valid_but_overlap_is_not() {
        let start = 1_790_000_000_000;
        let mut segments = vec![
            Segment {
                side: 1,
                start_utc_ms: start,
                end_utc_ms: start + 60_000,
                start_offset_minutes: 120,
                end_offset_minutes: 120,
            },
            Segment {
                side: 2,
                start_utc_ms: start + 120_000,
                end_utc_ms: start + 180_000,
                start_offset_minutes: 120,
                end_offset_minutes: 120,
            },
        ];
        assert!(fields(&segments, start, 120, start + 180_000).is_ok());
        segments[1].start_utc_ms = start + 59_999;
        assert!(fields(&segments, start, 120, start + 180_000).is_err());
    }

    #[test]
    fn published_pause_vector_matches_canonical_field_bytes() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/vectors/breast-segments-v1.json"))
                .unwrap();
        let case = &fixture["cases"][0];
        let segments: Vec<Segment> = serde_json::from_value(case["segments"].clone()).unwrap();
        let fields = fields(
            &segments,
            case["start_utc_ms"].as_i64().unwrap(),
            case["start_offset_minutes"].as_i64().unwrap() as i16,
            case["saved_at_ms"].as_i64().unwrap(),
        )
        .unwrap();
        let field = &fields.iter().find(|(id, _)| *id == 100).unwrap().1;
        let actual: String = crate::cbor::encode(field)
            .unwrap()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(actual, case["field_100_cbor_hex"].as_str().unwrap());
        let active: i64 = segments
            .iter()
            .map(|part| part.end_utc_ms - part.start_utc_ms)
            .sum();
        assert_eq!(active, case["expect_active_ms"].as_i64().unwrap());
        assert_eq!(
            segments[1].start_utc_ms - segments[0].end_utc_ms,
            case["expect_pause_ms"].as_i64().unwrap()
        );
    }
}
