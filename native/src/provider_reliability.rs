//! Provider stream, long-turn and structured-output reliability primitives.
//!
//! These are deterministic policies, not authority. They bound reconnect
//! attempts, enforce that a turn reservation is settled or released exactly
//! once, and limit schema-repair loops. A model response is still decoded by
//! the typed decoder; this module never accepts malformed output.

use std::collections::VecDeque;
use std::time::Duration;

pub const MAX_SSE_RECONNECT_ATTEMPTS: u32 = 8;
pub const DEFAULT_RECONNECT_BASE_MS: u64 = 250;
pub const DEFAULT_RECONNECT_MAX_MS: u64 = 30_000;
pub const MAX_STRUCTURED_REPAIRS: u8 = 2;
/// Bounded replay window. Event ids older than this many unique events may be
/// accepted again; the window bounds memory while catching normal SSE replay
/// duplication across reconnects.
pub const MAX_TRACKED_EVENT_IDS: usize = 4_096;

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
    seen_event_ids: VecDeque<String>,
}

impl SseCursor {
    pub fn new() -> Self {
        Self {
            last_event_id: None,
            reconnects: 0,
            seen_event_ids: VecDeque::new(),
        }
    }

    pub fn observe(&mut self, event_id: Option<&str>) -> Result<bool, String> {
        let Some(event_id) = event_id else {
            return Ok(false);
        };
        if event_id.trim().is_empty() || event_id.len() > 512 {
            return Err("SSE event id is empty or unbounded".into());
        }
        if self.seen_event_ids.iter().any(|seen| seen == event_id) {
            return Ok(false);
        }
        self.seen_event_ids.push_back(event_id.to_string());
        while self.seen_event_ids.len() > MAX_TRACKED_EVENT_IDS {
            self.seen_event_ids.pop_front();
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

    /// Start a new wait/turn with a fresh reconnect budget. The replay cursor
    /// (`last_event_id`) is retained: a later reconnect must still resume from
    /// the last event observed across turns.
    pub fn reset_reconnects(&mut self) {
        self.reconnects = 0;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalCompletion {
    /// Provider terminal evidence reconciled to a completed assistant message.
    Completed,
    /// Explicit provider failure, cancellation or protocol error.
    Failed,
    /// Stream cut/reconnect exhaustion or an unreconciled turn.
    Missing,
}

/// Candidate gate for provider-produced work.
///
/// A candidate requires an independently reconciled completed terminal, at
/// least one applied write and every frozen check passed. A reconnect, an HTTP
/// 200, a stream opening or a turn error after earlier checks passed is not
/// completion evidence.
pub fn candidate_allowed(
    terminal: TerminalCompletion,
    wrote: bool,
    all_checks_passed: bool,
) -> bool {
    terminal == TerminalCompletion::Completed && wrote && all_checks_passed
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

    pub fn max_repairs(&self) -> u8 {
        self.max_repairs
    }

    /// A valid structured response ends the current repair sequence. The next
    /// malformed response gets a fresh bounded repair budget.
    pub fn reset(&mut self) {
        self.repairs = 0;
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StructuredOutputError {
    #[error("structured-output repair {attempt}/{max_repairs} required after rejection: {detail}")]
    Repair {
        attempt: u8,
        max_repairs: u8,
        detail: String,
    },
    #[error("structured-output repair attempts exhausted after {attempts}: {detail}")]
    Exhausted { attempts: u8, detail: String },
}

/// Decode one machine-readable model result with a bounded repair sequence.
///
/// A malformed result returns `Repair` while the policy still allows another
/// attempt; the caller must submit the bounded repair prompt and try again. An
/// exhausted policy returns `Exhausted`, which is a typed failure, never a
/// free-text fallback or an accepted partial object.
pub fn decode_structured_output<T, F>(
    repair: &mut StructuredOutputRepair,
    decode: F,
) -> Result<T, StructuredOutputError>
where
    F: FnOnce() -> Result<T, String>,
{
    match decode() {
        Ok(value) => {
            repair.reset();
            Ok(value)
        }
        Err(detail) => {
            if repair.can_repair() {
                let attempt =
                    repair
                        .record_repair()
                        .map_err(|_| StructuredOutputError::Exhausted {
                            attempts: repair.attempts(),
                            detail: detail.clone(),
                        })?;
                Err(StructuredOutputError::Repair {
                    attempt,
                    max_repairs: repair.max_repairs(),
                    detail,
                })
            } else {
                Err(StructuredOutputError::Exhausted {
                    attempts: repair.attempts(),
                    detail,
                })
            }
        }
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
    fn cursor_deduplicates_nonconsecutive_event_ids_within_a_bounded_window() {
        let mut cursor = SseCursor::new();
        assert!(cursor.observe(Some("event-1")).unwrap());
        assert!(cursor.observe(Some("event-2")).unwrap());
        assert!(
            !cursor.observe(Some("event-1")).unwrap(),
            "a nonconsecutive replay must be suppressed"
        );
        assert!(cursor.observe(Some("event-3")).unwrap());
        assert_eq!(cursor.last_event_id.as_deref(), Some("event-3"));
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

    #[test]
    fn malformed_structured_output_is_repaired_then_accepted() {
        let mut repair = StructuredOutputRepair::new(MAX_STRUCTURED_REPAIRS).unwrap();
        let malformed =
            decode_structured_output(&mut repair, || Err::<&str, _>("truncated JSON".into()))
                .unwrap_err();
        assert_eq!(
            malformed,
            StructuredOutputError::Repair {
                attempt: 1,
                max_repairs: MAX_STRUCTURED_REPAIRS,
                detail: "truncated JSON".into(),
            }
        );
        let value = decode_structured_output(&mut repair, || Ok::<_, String>("valid")).unwrap();
        assert_eq!(value, "valid");
        assert_eq!(repair.attempts(), 0, "success resets the repair sequence");
    }

    #[test]
    fn structured_output_repair_exhaustion_is_a_typed_failure() {
        let mut repair = StructuredOutputRepair::new(1).unwrap();
        assert!(matches!(
            decode_structured_output(&mut repair, || Err::<(), _>("bad".into())),
            Err(StructuredOutputError::Repair { attempt: 1, .. })
        ));
        assert_eq!(
            decode_structured_output(&mut repair, || Err::<(), _>("still bad".into())),
            Err(StructuredOutputError::Exhausted {
                attempts: 1,
                detail: "still bad".into(),
            })
        );
    }

    #[test]
    fn cursor_window_is_bounded() {
        let mut cursor = SseCursor::new();
        for index in 0..MAX_TRACKED_EVENT_IDS + 8 {
            cursor.observe(Some(&format!("event-{index}"))).unwrap();
        }
        assert_eq!(cursor.seen_event_ids.len(), MAX_TRACKED_EVENT_IDS);
        assert_eq!(
            cursor.last_event_id,
            Some(format!("event-{}", MAX_TRACKED_EVENT_IDS + 7))
        );
    }

    #[test]
    fn reconnect_budget_resets_without_losing_the_replay_cursor() {
        let policy = SseReconnectPolicy::default();
        let mut cursor = SseCursor::new();
        cursor.observe(Some("event-7")).unwrap();
        for _ in 0..policy.max_attempts {
            cursor.reconnect(&policy).unwrap();
        }
        assert!(cursor.reconnect(&policy).is_err());
        cursor.reset_reconnects();
        assert_eq!(cursor.reconnects, 0);
        assert_eq!(cursor.last_event_id.as_deref(), Some("event-7"));
        cursor.reconnect(&policy).unwrap();
    }

    #[test]
    fn candidate_requires_completed_terminal_and_checks() {
        assert!(candidate_allowed(TerminalCompletion::Completed, true, true));
        assert!(!candidate_allowed(TerminalCompletion::Missing, true, true));
        assert!(!candidate_allowed(TerminalCompletion::Failed, true, true));
        assert!(!candidate_allowed(
            TerminalCompletion::Completed,
            false,
            true
        ));
        assert!(!candidate_allowed(
            TerminalCompletion::Completed,
            true,
            false
        ));
    }
}
