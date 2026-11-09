//! Provider stream, long-turn and structured-output reliability primitives.
//!
//! These are deterministic policies, not authority. They bound reconnect
//! attempts, enforce that a turn reservation is settled or released exactly
//! once, and limit schema-repair loops. A model response is still decoded by
//! the typed decoder; this module never accepts malformed output.

use std::time::Duration;

pub const MAX_SSE_RECONNECT_ATTEMPTS: u32 = 8;
pub const DEFAULT_RECONNECT_BASE_MS: u64 = 250;
pub const DEFAULT_RECONNECT_MAX_MS: u64 = 30_000;
pub const MAX_STRUCTURED_REPAIRS: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SseReconnectPolicy {
    pub max_attempts: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
}

impl Default for SseReconnectPolicy {
    fn default() -> Self {
        Self {
            max_attempts: MAX_SSE_RECONNECT_ATTEMPTS,
            base_delay_ms: DEFAULT_RECONNECT_BASE_MS,
            max_delay_ms: DEFAULT_RECONNECT_MAX_MS,
        }
    }
}

impl SseReconnectPolicy {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_attempts == 0
            || self.base_delay_ms == 0
            || self.max_delay_ms < self.base_delay_ms
            || self.max_delay_ms > 300_000
        {
            return Err("invalid SSE reconnect policy".into());
        }
        Ok(())
    }

    /// Exponential backoff capped by `max_delay_ms`. Attempt 1 is the first
    /// retry, not the initial connection.
    pub fn delay(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }
        let exponent = attempt.saturating_sub(1).min(20);
        let delay = self
            .base_delay_ms
            .saturating_mul(1_u64 << exponent)
            .min(self.max_delay_ms);
        Duration::from_millis(delay)
    }

    pub fn can_retry(&self, attempts: u32) -> bool {
        attempts < self.max_attempts
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseCursor {
    pub last_event_id: Option<String>,
    pub reconnects: u32,
}

impl SseCursor {
    pub fn new() -> Self {
        Self {
            last_event_id: None,
            reconnects: 0,
        }
    }

    pub fn observe(&mut self, event_id: Option<&str>) -> Result<bool, String> {
        let Some(event_id) = event_id else {
            return Ok(false);
        };
        if event_id.trim().is_empty() || event_id.len() > 512 {
            return Err("SSE event id is empty or unbounded".into());
        }
        if self.last_event_id.as_deref() == Some(event_id) {
            return Ok(false);
        }
        self.last_event_id = Some(event_id.to_string());
        Ok(true)
    }

    pub fn reconnect(&mut self, policy: &SseReconnectPolicy) -> Result<Duration, String> {
        policy.validate()?;
        if !policy.can_retry(self.reconnects) {
            return Err("SSE reconnect attempts exhausted".into());
        }
        self.reconnects = self.reconnects.saturating_add(1);
        Ok(policy.delay(self.reconnects))
    }
}

impl Default for SseCursor {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnReservation {
    reserved: bool,
    settled: bool,
    released: bool,
}

impl TurnReservation {
    pub fn reserved() -> Self {
        Self {
            reserved: true,
            settled: false,
            released: false,
        }
    }

    pub fn settle(&mut self) -> Result<(), String> {
        if !self.reserved || self.settled || self.released {
            return Err("turn reservation is not settleable".into());
        }
        self.settled = true;
        Ok(())
    }

    pub fn release(&mut self) -> Result<(), String> {
        if !self.reserved || self.settled || self.released {
            return Err("turn reservation is not releasable".into());
        }
        self.released = true;
        Ok(())
    }

    pub fn is_terminal(&self) -> bool {
        self.settled || self.released
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredOutputRepair {
    repairs: u8,
    max_repairs: u8,
}

impl StructuredOutputRepair {
    pub fn new(max_repairs: u8) -> Result<Self, String> {
        if max_repairs > MAX_STRUCTURED_REPAIRS {
            return Err("structured-output repair bound is too large".into());
        }
        Ok(Self {
            repairs: 0,
            max_repairs,
        })
    }

    pub fn can_repair(&self) -> bool {
        self.repairs < self.max_repairs
    }

    pub fn record_repair(&mut self) -> Result<u8, String> {
        if !self.can_repair() {
            return Err("structured-output repair attempts exhausted".into());
        }
        self.repairs = self.repairs.saturating_add(1);
        Ok(self.repairs)
    }

    pub fn attempts(&self) -> u8 {
        self.repairs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_backoff_is_bounded_and_exhausts() {
        let policy = SseReconnectPolicy::default();
        policy.validate().unwrap();
        assert_eq!(policy.delay(0), Duration::ZERO);
        assert_eq!(policy.delay(1), Duration::from_millis(250));
        assert_eq!(policy.delay(2), Duration::from_millis(500));
        assert_eq!(policy.delay(100), Duration::from_millis(30_000));
        let mut cursor = SseCursor::new();
        for attempt in 1..=policy.max_attempts {
            assert!(cursor.reconnect(&policy).is_ok(), "attempt {attempt}");
        }
        assert!(cursor.reconnect(&policy).is_err());
    }

    #[test]
    fn cursor_deduplicates_event_ids() {
        let mut cursor = SseCursor::new();
        assert!(cursor.observe(Some("event-1")).unwrap());
        assert!(!cursor.observe(Some("event-1")).unwrap());
        assert!(cursor.observe(Some("event-2")).unwrap());
        assert!(cursor.observe(Some("")).is_err());
    }

    #[test]
    fn reservation_is_settled_or_released_exactly_once() {
        let mut reservation = TurnReservation::reserved();
        reservation.settle().unwrap();
        assert!(reservation.is_terminal());
        assert!(reservation.release().is_err());
        let mut released = TurnReservation::reserved();
        released.release().unwrap();
        assert!(released.settle().is_err());
    }

    #[test]
    fn structured_output_repairs_are_bounded() {
        let mut repair = StructuredOutputRepair::new(MAX_STRUCTURED_REPAIRS).unwrap();
        assert_eq!(repair.record_repair().unwrap(), 1);
        assert_eq!(repair.record_repair().unwrap(), 2);
        assert!(!repair.can_repair());
        assert!(repair.record_repair().is_err());
        assert!(StructuredOutputRepair::new(MAX_STRUCTURED_REPAIRS + 1).is_err());
    }
}
