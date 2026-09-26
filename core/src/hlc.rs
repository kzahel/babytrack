//! Per-Family local HLC generation. Relay order, not this clock, wins fields.

use crate::operation::Hlc;

const MAX_COUNTER: u32 = 1_000_000;
const FUTURE_LIMIT_MS: i128 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clock {
    family_id: [u8; 16],
    device_id: [u8; 16],
    last_local: Option<Hlc>,
    max_received: Option<Hlc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Next {
    pub stamp: Hlc,
    pub anomaly: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observation {
    Accepted,
    FutureAnomaly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidLocalDevice,
    CounterOutOfRange,
}

impl Clock {
    pub fn new(family_id: [u8; 16], device_id: [u8; 16]) -> Self {
        Self {
            family_id,
            device_id,
            last_local: None,
            max_received: None,
        }
    }

    /// Rehydrate this Family's clock state inside the same storage boundary
    /// that holds its operation log and outbox.
    pub fn restore(
        family_id: [u8; 16],
        device_id: [u8; 16],
        last_local: Option<Hlc>,
        max_received: Option<Hlc>,
    ) -> Result<Self, Error> {
        if last_local
            .as_ref()
            .is_some_and(|stamp| stamp.device_id != device_id)
        {
            return Err(Error::InvalidLocalDevice);
        }
        if last_local
            .iter()
            .chain(max_received.iter())
            .any(|stamp| stamp.counter > MAX_COUNTER)
        {
            return Err(Error::CounterOutOfRange);
        }
        Ok(Self {
            family_id,
            device_id,
            last_local,
            max_received,
        })
    }

    pub fn family_id(&self) -> [u8; 16] {
        self.family_id
    }

    pub fn last_local(&self) -> Option<&Hlc> {
        self.last_local.as_ref()
    }

    pub fn max_received(&self) -> Option<&Hlc> {
        self.max_received.as_ref()
    }

    /// Observe an already verified remote operation at its relay commit time.
    /// The caller records future anomalies for the sync integrity UI.
    pub fn observe_remote(&mut self, stamp: &Hlc, commit_ms: i64) -> Result<Observation, Error> {
        if stamp.counter > MAX_COUNTER {
            return Err(Error::CounterOutOfRange);
        }
        if i128::from(stamp.wall_ms) - i128::from(commit_ms) > FUTURE_LIMIT_MS {
            return Ok(Observation::FutureAnomaly);
        }
        if self.max_received.as_ref().is_none_or(|current| {
            (stamp.wall_ms, stamp.counter) > (current.wall_ms, current.counter)
        }) {
            self.max_received = Some(stamp.clone());
        }
        Ok(Observation::Accepted)
    }

    /// Allocate the next local stamp. Persist this state with the operation.
    pub fn next(&mut self, now_ms: i64) -> Next {
        let local = self.last_local.as_ref();
        let remote = self.max_received.as_ref();
        let wall_ms = now_ms
            .max(local.map_or(i64::MIN, |stamp| stamp.wall_ms))
            .max(remote.map_or(i64::MIN, |stamp| stamp.wall_ms));
        let local_at_wall = local.is_some_and(|stamp| stamp.wall_ms == wall_ms);
        let remote_at_wall = remote.is_some_and(|stamp| stamp.wall_ms == wall_ms);
        let counter = match (local_at_wall, remote_at_wall) {
            (true, true) => local.unwrap().counter.max(remote.unwrap().counter) + 1,
            (true, false) => local.unwrap().counter + 1,
            (false, true) => remote.unwrap().counter + 1,
            (false, false) => 0,
        };
        let (wall_ms, counter, anomaly) = if counter > MAX_COUNTER {
            if let Some(advanced) = wall_ms.checked_add(1) {
                (advanced, 0, false)
            } else if let Some(previous) = &self.last_local {
                return Next {
                    stamp: previous.clone(),
                    anomaly: true,
                };
            } else {
                (i64::MAX, MAX_COUNTER, true)
            }
        } else {
            (wall_ms, counter, false)
        };
        let stamp = Hlc {
            wall_ms,
            counter,
            device_id: self.device_id,
        };
        self.last_local = Some(stamp.clone());
        Next { stamp, anomaly }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(wall_ms: i64, counter: u32, device: u8) -> Hlc {
        Hlc {
            wall_ms,
            counter,
            device_id: [device; 16],
        }
    }

    #[test]
    fn local_and_remote_ties_follow_the_published_clock_rule() {
        let mut clock = Clock::new([1; 16], [2; 16]);
        assert_eq!(clock.next(100).stamp, stamp(100, 0, 2));
        assert_eq!(clock.next(100).stamp, stamp(100, 1, 2));
        assert_eq!(
            clock.observe_remote(&stamp(100, 5, 3), 100),
            Ok(Observation::Accepted)
        );
        assert_eq!(clock.next(99).stamp, stamp(100, 6, 2));
        assert_eq!(clock.next(101).stamp, stamp(101, 0, 2));
        assert_eq!(clock.family_id(), [1; 16]);
    }

    #[test]
    fn a_far_future_remote_stamp_does_not_poison_local_edits() {
        let mut clock = Clock::new([1; 16], [2; 16]);
        let future = stamp(86_400_001, MAX_COUNTER, 3);
        assert_eq!(
            clock.observe_remote(&future, 0),
            Ok(Observation::FutureAnomaly)
        );
        assert_eq!(clock.max_received(), None);
        assert_eq!(clock.next(1).stamp, stamp(1, 0, 2));
        assert_eq!(
            clock.observe_remote(&stamp(86_400_000, 0, 3), 0),
            Ok(Observation::Accepted)
        );
    }

    #[test]
    fn counter_rolls_wall_and_max_wall_preserves_the_edit() {
        let mut clock =
            Clock::restore([1; 16], [2; 16], Some(stamp(100, MAX_COUNTER, 2)), None).unwrap();
        assert_eq!(
            clock.next(100),
            Next {
                stamp: stamp(101, 0, 2),
                anomaly: false
            }
        );

        let prior = stamp(i64::MAX, MAX_COUNTER, 2);
        let mut at_limit = Clock::restore([1; 16], [2; 16], Some(prior.clone()), None).unwrap();
        assert_eq!(
            at_limit.next(i64::MAX),
            Next {
                stamp: prior.clone(),
                anomaly: true
            }
        );
        assert_eq!(at_limit.last_local(), Some(&prior));
    }

    #[test]
    fn restored_clock_rejects_invalid_state() {
        assert_eq!(
            Clock::restore([1; 16], [2; 16], Some(stamp(0, 0, 3)), None),
            Err(Error::InvalidLocalDevice)
        );
        assert_eq!(
            Clock::restore([1; 16], [2; 16], None, Some(stamp(0, MAX_COUNTER + 1, 3))),
            Err(Error::CounterOutOfRange)
        );
    }
}
