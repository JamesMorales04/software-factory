//! Deterministic provider-session state machine.
//!
//! This is transport-independent. It validates exact provider/model/session
//! identity, monotonic events, permission decisions, cancellation, terminal
//! status and usage accounting. It never contacts a provider and never grants
//! Factory authority.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Codex,
    OpenCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Starting,
    Active,
    WaitingPermission,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
    CleanupPending,
    Cleaned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderEvent {
    SessionStarted {
        sequence: u64,
        session_id: String,
    },
    TurnStarted {
        sequence: u64,
        turn_id: String,
    },
    MessageDelta {
        sequence: u64,
        text: String,
    },
    ToolRequested {
        sequence: u64,
        call_id: String,
        tool: String,
    },
    PermissionRequested {
        sequence: u64,
        call_id: String,
        permission: String,
    },
    PermissionDecided {
        sequence: u64,
        call_id: String,
        decision: PermissionDecision,
    },
    Usage {
        sequence: u64,
        usage: Usage,
    },
    TurnTerminal {
        sequence: u64,
        turn_id: String,
        status: TerminalStatus,
    },
    Error {
        sequence: u64,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalStatus {
    Completed,
    Failed,
    Interrupted,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SessionError {
    #[error("session identity mismatch")]
    IdentityMismatch,
    #[error("event sequence is not strictly increasing")]
    Sequence,
    #[error("event is invalid for the current session state")]
    InvalidTransition,
    #[error("session is already terminal")]
    Terminal,
    #[error("session has no pending permission request")]
    NoPermission,
    #[error("permission request does not match")]
    PermissionMismatch,
    #[error("usage accounting overflow")]
    UsageOverflow,
    #[error("cleanup is not confirmed")]
    CleanupUnconfirmed,
}

#[derive(Debug, Clone)]
pub struct ProviderSession {
    pub provider: ProviderKind,
    pub model: String,
    pub mode: String,
    pub session_id: Option<String>,
    pub active_turn_id: Option<String>,
    pub state: SessionState,
    pub last_sequence: Option<u64>,
    pub usage: Usage,
    pub streamed_text: String,
    pub pending_permission: Option<(String, String)>,
    pub seen_events: BTreeMap<u64, ProviderEvent>,
    pub cleanup_receipt: Option<String>,
}

impl ProviderSession {
    pub fn new(provider: ProviderKind, model: &str, mode: &str) -> Result<Self, SessionError> {
        if model.trim().is_empty()
            || mode.trim().is_empty()
            || model.chars().any(char::is_whitespace)
            || mode.chars().any(char::is_whitespace)
        {
            return Err(SessionError::IdentityMismatch);
        }
        Ok(Self {
            provider,
            model: model.into(),
            mode: mode.into(),
            session_id: None,
            active_turn_id: None,
            state: SessionState::Starting,
            last_sequence: None,
            usage: Usage::default(),
            streamed_text: String::new(),
            pending_permission: None,
            seen_events: BTreeMap::new(),
            cleanup_receipt: None,
        })
    }

    pub fn apply(&mut self, event: &ProviderEvent) -> Result<(), SessionError> {
        let sequence = event.sequence();
        if let Some(previous) = self.seen_events.get(&sequence) {
            return if previous == event {
                Ok(())
            } else {
                Err(SessionError::Sequence)
            };
        }
        if self.last_sequence.is_some_and(|last| sequence <= last) {
            return Err(SessionError::Sequence);
        }
        if self.is_terminal() {
            return Err(SessionError::Terminal);
        }
        match event {
            ProviderEvent::SessionStarted {
                sequence,
                session_id,
            } => {
                if *sequence != 1
                    || self.state != SessionState::Starting
                    || self.session_id.is_some()
                    || session_id.trim().is_empty()
                {
                    return Err(SessionError::InvalidTransition);
                }
                self.session_id = Some(session_id.clone());
                self.state = SessionState::Active;
            }
            ProviderEvent::TurnStarted { turn_id, .. } => {
                self.require_active()?;
                if turn_id.trim().is_empty() || self.active_turn_id.is_some() {
                    return Err(SessionError::InvalidTransition);
                }
                self.active_turn_id = Some(turn_id.clone());
            }
            ProviderEvent::MessageDelta { text, .. } => {
                self.require_active()?;
                if text.len() > 64 * 1024 {
                    return Err(SessionError::InvalidTransition);
                }
                self.streamed_text.push_str(text);
            }
            ProviderEvent::ToolRequested { .. } => {
                self.require_active()?;
            }
            ProviderEvent::PermissionRequested {
                call_id,
                permission,
                ..
            } => {
                self.require_active()?;
                if self.pending_permission.is_some() || call_id.is_empty() || permission.is_empty()
                {
                    return Err(SessionError::InvalidTransition);
                }
                self.pending_permission = Some((call_id.clone(), permission.clone()));
                self.state = SessionState::WaitingPermission;
            }
            ProviderEvent::PermissionDecided {
                call_id, decision, ..
            } => {
                let Some((pending_call, _)) = self.pending_permission.clone() else {
                    return Err(SessionError::NoPermission);
                };
                if pending_call != *call_id {
                    return Err(SessionError::PermissionMismatch);
                }
                self.pending_permission = None;
                self.state = SessionState::Active;
                if *decision == PermissionDecision::Deny {
                    // A denial is an authoritative provider-side decision; the
                    // caller must not synthesize tool execution after it.
                    self.streamed_text.push_str("\n[permission denied]");
                }
            }
            ProviderEvent::Usage { usage, .. } => {
                self.require_active_or_cancelling()?;
                let next = Usage {
                    input_tokens: self
                        .usage
                        .input_tokens
                        .checked_add(usage.input_tokens)
                        .ok_or(SessionError::UsageOverflow)?,
                    output_tokens: self
                        .usage
                        .output_tokens
                        .checked_add(usage.output_tokens)
                        .ok_or(SessionError::UsageOverflow)?,
                    total_tokens: self
                        .usage
                        .total_tokens
                        .checked_add(usage.total_tokens)
                        .ok_or(SessionError::UsageOverflow)?,
                    cost_micros: self
                        .usage
                        .cost_micros
                        .checked_add(usage.cost_micros)
                        .ok_or(SessionError::UsageOverflow)?,
                };
                self.usage = next;
            }
            ProviderEvent::TurnTerminal {
                turn_id, status, ..
            } => {
                if !matches!(self.state, SessionState::Active | SessionState::Cancelling) {
                    return Err(SessionError::InvalidTransition);
                }
                if self.active_turn_id.as_deref() != Some(turn_id.as_str()) {
                    return Err(SessionError::IdentityMismatch);
                }
                if self.pending_permission.is_some() {
                    return Err(SessionError::InvalidTransition);
                }
                self.state = match status {
                    TerminalStatus::Completed => SessionState::Completed,
                    TerminalStatus::Failed => SessionState::Failed,
                    TerminalStatus::Interrupted => SessionState::Cancelled,
                };
                self.state = SessionState::CleanupPending;
                self.last_sequence = Some(sequence);
                self.seen_events.insert(sequence, event.clone());
                return Ok(());
            }
            ProviderEvent::Error { .. } => {
                self.state = SessionState::Failed;
                self.state = SessionState::CleanupPending;
                self.last_sequence = Some(sequence);
                self.seen_events.insert(sequence, event.clone());
                return Ok(());
            }
        }
        self.last_sequence = Some(sequence);
        self.seen_events.insert(sequence, event.clone());
        Ok(())
    }

    pub fn decide_permission(
        &mut self,
        call_id: &str,
        decision: PermissionDecision,
    ) -> Result<ProviderEvent, SessionError> {
        let (pending_call, _) = self
            .pending_permission
            .clone()
            .ok_or(SessionError::NoPermission)?;
        if pending_call != call_id {
            return Err(SessionError::PermissionMismatch);
        }
        let sequence = self.last_sequence.unwrap_or(0) + 1;
        let event = ProviderEvent::PermissionDecided {
            sequence,
            call_id: call_id.into(),
            decision,
        };
        self.apply(&event)?;
        Ok(event)
    }

    pub fn request_cancel(&mut self) -> Result<(), SessionError> {
        if self.is_terminal() {
            return Err(SessionError::Terminal);
        }
        if !matches!(
            self.state,
            SessionState::Active | SessionState::WaitingPermission
        ) {
            return Err(SessionError::InvalidTransition);
        }
        self.state = SessionState::Cancelling;
        self.pending_permission = None;
        Ok(())
    }

    pub fn reconnect(&mut self, session_id: &str) -> Result<(), SessionError> {
        if self.session_id.as_deref() != Some(session_id) {
            return Err(SessionError::IdentityMismatch);
        }
        if self.is_terminal() {
            return Err(SessionError::Terminal);
        }
        Ok(())
    }

    pub fn confirm_cleanup(&mut self, receipt: &str) -> Result<(), SessionError> {
        if self.state != SessionState::CleanupPending {
            return Err(SessionError::CleanupUnconfirmed);
        }
        if receipt.trim().is_empty() {
            return Err(SessionError::CleanupUnconfirmed);
        }
        self.cleanup_receipt = Some(receipt.into());
        self.state = SessionState::Cleaned;
        Ok(())
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            SessionState::CleanupPending | SessionState::Cleaned
        )
    }

    fn require_active(&self) -> Result<(), SessionError> {
        if self.state != SessionState::Active {
            return Err(SessionError::InvalidTransition);
        }
        Ok(())
    }

    fn require_active_or_cancelling(&self) -> Result<(), SessionError> {
        if !matches!(self.state, SessionState::Active | SessionState::Cancelling) {
            return Err(SessionError::InvalidTransition);
        }
        Ok(())
    }
}

impl ProviderEvent {
    pub fn sequence(&self) -> u64 {
        match self {
            Self::SessionStarted { sequence, .. }
            | Self::TurnStarted { sequence, .. }
            | Self::MessageDelta { sequence, .. }
            | Self::ToolRequested { sequence, .. }
            | Self::PermissionRequested { sequence, .. }
            | Self::PermissionDecided { sequence, .. }
            | Self::Usage { sequence, .. }
            | Self::TurnTerminal { sequence, .. }
            | Self::Error { sequence, .. } => *sequence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started() -> ProviderSession {
        let mut session =
            ProviderSession::new(ProviderKind::OpenCode, "deepseek-v4.1-flash", "build").unwrap();
        session
            .apply(&ProviderEvent::SessionStarted {
                sequence: 1,
                session_id: "ses_1".into(),
            })
            .unwrap();
        session
    }

    #[test]
    fn streams_usage_and_rejects_duplicate_or_out_of_order_events() {
        let mut session = started();
        session
            .apply(&ProviderEvent::MessageDelta {
                sequence: 2,
                text: "hello".into(),
            })
            .unwrap();
        session
            .apply(&ProviderEvent::MessageDelta {
                sequence: 2,
                text: "hello".into(),
            })
            .unwrap();
        assert_eq!(session.streamed_text, "hello");
        session
            .apply(&ProviderEvent::Usage {
                sequence: 3,
                usage: Usage {
                    input_tokens: 10,
                    output_tokens: 5,
                    total_tokens: 15,
                    cost_micros: 7,
                },
            })
            .unwrap();
        assert_eq!(session.usage.total_tokens, 15);
        session
            .apply(&ProviderEvent::MessageDelta {
                sequence: 10,
                text: "later".into(),
            })
            .unwrap();
        assert_eq!(
            session.apply(&ProviderEvent::MessageDelta {
                sequence: 5,
                text: "out of order".into()
            }),
            Err(SessionError::Sequence)
        );
    }

    #[test]
    fn rejected_events_do_not_become_accepted_replays() {
        let mut session = started();
        let invalid = ProviderEvent::TurnTerminal {
            sequence: 2,
            turn_id: "turn-1".into(),
            status: TerminalStatus::Completed,
        };
        assert_eq!(session.apply(&invalid), Err(SessionError::IdentityMismatch));
        assert_eq!(session.apply(&invalid), Err(SessionError::IdentityMismatch));
        assert_eq!(session.last_sequence, Some(1));
        assert!(!session.seen_events.contains_key(&2));

        let accepted = ProviderEvent::TurnStarted {
            sequence: 2,
            turn_id: "turn-1".into(),
        };
        session.apply(&accepted).unwrap();
        assert_eq!(session.apply(&accepted), Ok(()));
        assert_eq!(session.apply(&invalid), Err(SessionError::Sequence));
    }

    #[test]
    fn rejected_usage_overflow_keeps_accounting_and_sequence_unchanged() {
        let mut session = started();
        session.usage.cost_micros = u64::MAX;
        let event = ProviderEvent::Usage {
            sequence: 2,
            usage: Usage {
                input_tokens: 1,
                output_tokens: 1,
                total_tokens: 2,
                cost_micros: 1,
            },
        };
        assert_eq!(session.apply(&event), Err(SessionError::UsageOverflow));
        assert_eq!(session.usage.input_tokens, 0);
        assert_eq!(session.usage.output_tokens, 0);
        assert_eq!(session.usage.total_tokens, 0);
        assert_eq!(session.usage.cost_micros, u64::MAX);
        assert_eq!(session.last_sequence, Some(1));
        assert_eq!(session.apply(&event), Err(SessionError::UsageOverflow));
    }

    #[test]
    fn permission_requests_require_an_exact_decision() {
        let mut session = started();
        session
            .apply(&ProviderEvent::PermissionRequested {
                sequence: 2,
                call_id: "call-1".into(),
                permission: "shell".into(),
            })
            .unwrap();
        assert_eq!(session.state, SessionState::WaitingPermission);
        assert_eq!(
            session.decide_permission("wrong", PermissionDecision::Allow),
            Err(SessionError::PermissionMismatch)
        );
        let event = session
            .decide_permission("call-1", PermissionDecision::Deny)
            .unwrap();
        assert_eq!(
            event,
            ProviderEvent::PermissionDecided {
                sequence: 3,
                call_id: "call-1".into(),
                decision: PermissionDecision::Deny
            }
        );
        assert_eq!(session.state, SessionState::Active);
        assert!(session.streamed_text.contains("permission denied"));
    }

    #[test]
    fn cancellation_is_not_terminal_until_provider_interrupts() {
        let mut session = started();
        session
            .apply(&ProviderEvent::TurnStarted {
                sequence: 2,
                turn_id: "turn-1".into(),
            })
            .unwrap();
        session.request_cancel().unwrap();
        assert_eq!(session.state, SessionState::Cancelling);
        assert!(!session.is_terminal());
        session
            .apply(&ProviderEvent::TurnTerminal {
                sequence: 3,
                turn_id: "turn-1".into(),
                status: TerminalStatus::Interrupted,
            })
            .unwrap();
        assert_eq!(session.state, SessionState::CleanupPending);
        assert_eq!(
            session.confirm_cleanup(""),
            Err(SessionError::CleanupUnconfirmed)
        );
        session.confirm_cleanup("deleted/404").unwrap();
        assert_eq!(session.state, SessionState::Cleaned);
    }

    #[test]
    fn terminal_event_must_match_the_started_turn() {
        let mut session = started();
        session
            .apply(&ProviderEvent::TurnStarted {
                sequence: 2,
                turn_id: "turn-1".into(),
            })
            .unwrap();
        assert_eq!(
            session.apply(&ProviderEvent::TurnTerminal {
                sequence: 3,
                turn_id: "other-turn".into(),
                status: TerminalStatus::Completed,
            }),
            Err(SessionError::IdentityMismatch)
        );
        assert_eq!(session.state, SessionState::Active);
    }

    #[test]
    fn failed_terminal_requires_cleanup_receipt() {
        let mut session = started();
        session
            .apply(&ProviderEvent::TurnStarted {
                sequence: 2,
                turn_id: "turn-1".into(),
            })
            .unwrap();
        session
            .apply(&ProviderEvent::Error {
                sequence: 3,
                message: "provider failed".into(),
            })
            .unwrap();
        assert_eq!(session.state, SessionState::CleanupPending);
        session.confirm_cleanup("provider absent").unwrap();
        assert_eq!(session.state, SessionState::Cleaned);
    }
}
