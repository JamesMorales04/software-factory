//! Incremental, bounded-memory parser for the Server-Sent Events wire format.
//!
//! This parses framing only. It does not provide reconnect behavior or imply
//! that the event stream is a durable log.

use serde_json::Value;
use std::fmt;

/// One complete SSE event. Events without a `data` field are not dispatched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseError {
    RecordTooLarge {
        limit: usize,
    },
    InvalidUtf8,
    InvalidEventJson,
    MissingEventType,
    InvalidEventProperties,
    MissingSessionIdentity,
    SessionIdentityMismatch {
        expected: String,
        found: String,
    },
    SessionPayloadMismatch {
        session_id: String,
        payload_id: String,
    },
}

impl fmt::Display for SseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RecordTooLarge { limit } => {
                write!(f, "SSE record exceeds configured limit of {limit} bytes")
            }
            Self::InvalidUtf8 => f.write_str("SSE field is not valid UTF-8"),
            Self::InvalidEventJson => f.write_str("OpenCode SSE data is not JSON"),
            Self::MissingEventType => f.write_str("OpenCode SSE event has no type"),
            Self::InvalidEventProperties => {
                f.write_str("OpenCode SSE event properties are invalid")
            }
            Self::MissingSessionIdentity => {
                f.write_str("OpenCode SSE event has no session identity")
            }
            Self::SessionIdentityMismatch { expected, found } => write!(
                f,
                "OpenCode SSE session identity mismatch: expected {expected}, found {found}"
            ),
            Self::SessionPayloadMismatch {
                session_id,
                payload_id,
            } => write!(
                f,
                "OpenCode SSE session payload identity mismatch: event {session_id}, payload {payload_id}"
            ),
        }
    }
}

impl std::error::Error for SseError {}

/// The stable identity envelope shared by OpenCode session events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCodeEvent {
    pub kind: String,
    pub session_id: Option<String>,
}

impl OpenCodeEvent {
    pub fn parse(data: &str) -> Result<Self, SseError> {
        let value: Value = serde_json::from_str(data).map_err(|_| SseError::InvalidEventJson)?;
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or(SseError::MissingEventType)?
            .to_owned();
        let properties = value.get("properties");
        let session_id = match properties {
            Some(Value::Object(properties)) => properties
                .get("sessionID")
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_owned)
                        .ok_or(SseError::InvalidEventProperties)
                })
                .transpose()?,
            Some(Value::Null) | None => None,
            Some(_) => return Err(SseError::InvalidEventProperties),
        };
        // Only session-level events carry a session object whose `info.id`
        // must equal `sessionID`. Message events carry a message `info.id`
        // (`msg_*`) inside the same envelope and are not a session identity
        // contradiction.
        if kind.starts_with("session.")
            && let (Some(session_id), Some(payload_id)) = (
                session_id.as_deref(),
                properties
                    .and_then(|properties| properties.get("info"))
                    .and_then(|info| info.get("id"))
                    .and_then(Value::as_str),
            )
            && session_id != payload_id
        {
            return Err(SseError::SessionPayloadMismatch {
                session_id: session_id.to_owned(),
                payload_id: payload_id.to_owned(),
            });
        }
        Ok(Self { kind, session_id })
    }

    pub fn require_session(&self, expected: &str) -> Result<(), SseError> {
        let found = self
            .session_id
            .as_deref()
            .ok_or(SseError::MissingSessionIdentity)?;
        if found != expected {
            return Err(SseError::SessionIdentityMismatch {
                expected: expected.to_owned(),
                found: found.to_owned(),
            });
        }
        Ok(())
    }
}

/// Incrementally parses SSE bytes. `max_record_bytes` bounds the current line
/// and accumulated fields for a single event; emitted events are returned to
/// the caller and are not retained by this parser.
pub struct SseParser {
    max_record_bytes: usize,
    line: Vec<u8>,
    record_bytes: usize,
    event: Option<String>,
    data: Vec<String>,
    id: Option<String>,
    skip_lf: bool,
}

impl SseParser {
    pub fn new(max_record_bytes: usize) -> Self {
        Self {
            max_record_bytes,
            line: Vec::new(),
            record_bytes: 0,
            event: None,
            data: Vec::new(),
            id: None,
            skip_lf: false,
        }
    }

    /// Feed an arbitrary byte chunk and return all events completed in it.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>, SseError> {
        let mut emitted = Vec::new();
        for &byte in chunk {
            if self.skip_lf {
                self.skip_lf = false;
                if byte == b'\n' {
                    continue;
                }
            }
            match byte {
                b'\r' => {
                    self.finish_line(&mut emitted)?;
                    self.skip_lf = true;
                }
                b'\n' => self.finish_line(&mut emitted)?,
                _ => {
                    if self.line.len() >= self.max_record_bytes {
                        return Err(SseError::RecordTooLarge {
                            limit: self.max_record_bytes,
                        });
                    }
                    self.line.push(byte);
                }
            }
        }
        Ok(emitted)
    }

    fn finish_line(&mut self, emitted: &mut Vec<SseEvent>) -> Result<(), SseError> {
        if self.line.is_empty() {
            if !self.data.is_empty() {
                emitted.push(SseEvent {
                    event: self.event.take(),
                    data: self.data.join("\n"),
                    id: self.id.take(),
                });
            } else {
                self.event = None;
                self.id = None;
            }
            self.data.clear();
            self.record_bytes = 0;
            return Ok(());
        }

        self.record_bytes = self
            .record_bytes
            .saturating_add(self.line.len().saturating_add(1));
        if self.record_bytes > self.max_record_bytes {
            return Err(SseError::RecordTooLarge {
                limit: self.max_record_bytes,
            });
        }
        let line_bytes = std::mem::take(&mut self.line);
        let line = std::str::from_utf8(&line_bytes).map_err(|_| SseError::InvalidUtf8)?;
        if line.starts_with(':') {
            return Ok(());
        }

        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "event" => self.event = Some(value.to_owned()),
            // The SSE standard ignores IDs containing NUL.
            "id" if !value.contains('\0') => self.id = Some(value.to_owned()),
            "data" => self.data.push(value.to_owned()),
            // Retry and extension fields are intentionally not interpreted.
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_split_crlf_and_multiline_data() {
        let mut parser = SseParser::new(128);
        assert!(parser.push(b"event: message\r").unwrap().is_empty());
        assert!(
            parser
                .push(b"\ndata: first\r\ndata: second\r")
                .unwrap()
                .is_empty()
        );
        let events = parser.push(b"\nid: cursor\r\n\r\n").unwrap();
        assert_eq!(
            events,
            vec![SseEvent {
                event: Some("message".into()),
                data: "first\nsecond".into(),
                id: Some("cursor".into()),
            }]
        );
    }

    #[test]
    fn parses_consecutive_reconnect_frames_and_ignores_comments() {
        let mut parser = SseParser::new(128);
        let events = parser
            .push(b": connected\n\ndata: one\nid: a\n\ndata: two\nid: b\n\n")
            .unwrap();
        assert_eq!(
            events,
            vec![
                SseEvent {
                    event: None,
                    data: "one".into(),
                    id: Some("a".into())
                },
                SseEvent {
                    event: None,
                    data: "two".into(),
                    id: Some("b".into())
                },
            ]
        );
    }

    #[test]
    fn every_byte_chunk_boundary_is_supported() {
        let input = "event: update\r\ndata: café\ndata: line 2\r\nid: 9\n\r\n".as_bytes();
        let mut parser = SseParser::new(128);
        let mut all = Vec::new();
        for byte in input {
            all.extend(parser.push(std::slice::from_ref(byte)).unwrap());
        }
        assert_eq!(
            all,
            vec![SseEvent {
                event: Some("update".into()),
                data: "café\nline 2".into(),
                id: Some("9".into()),
            }]
        );
    }

    #[test]
    fn rejects_oversized_records_with_explicit_limit() {
        let mut parser = SseParser::new(8);
        let error = parser.push(b"data: 12345\n").unwrap_err();
        assert_eq!(error, SseError::RecordTooLarge { limit: 8 });
    }

    #[test]
    fn parses_session_event_identity_and_payload_identity() {
        let event = OpenCodeEvent::parse(
            r#"{"id":"evt_1","type":"session.created","properties":{"sessionID":"ses_1","info":{"id":"ses_1"}}}"#,
        )
        .unwrap();
        assert_eq!(event.kind, "session.created");
        assert_eq!(event.session_id.as_deref(), Some("ses_1"));
        event.require_session("ses_1").unwrap();
    }

    #[test]
    fn rejects_session_event_for_different_expected_identity() {
        let event = OpenCodeEvent::parse(
            r#"{"type":"session.deleted","properties":{"sessionID":"ses_other","info":{"id":"ses_other"}}}"#,
        )
        .unwrap();
        assert!(matches!(
            event.require_session("ses_expected"),
            Err(SseError::SessionIdentityMismatch { .. })
        ));
    }

    #[test]
    fn rejects_disagreeing_session_and_payload_ids() {
        assert!(matches!(
            OpenCodeEvent::parse(
                r#"{"type":"session.updated","properties":{"sessionID":"ses_event","info":{"id":"ses_payload"}}}"#,
            ),
            Err(SseError::SessionPayloadMismatch { .. })
        ));
    }

    #[test]
    fn message_event_identity_is_not_treated_as_a_session_identity() {
        let event = OpenCodeEvent::parse(
            r#"{"type":"message.updated","properties":{"sessionID":"ses_1","info":{"id":"msg_1","role":"assistant"}}}"#,
        )
        .unwrap();
        assert_eq!(event.kind, "message.updated");
        assert_eq!(event.session_id.as_deref(), Some("ses_1"));
        event.require_session("ses_1").unwrap();
    }

    #[test]
    fn global_connection_event_does_not_claim_session_identity() {
        let event = OpenCodeEvent::parse(r#"{"type":"server.connected"}"#).unwrap();
        assert_eq!(event.session_id, None);
        assert_eq!(
            event.require_session("ses_1"),
            Err(SseError::MissingSessionIdentity)
        );
    }

    #[test]
    fn rejects_malformed_or_untyped_event_data() {
        assert_eq!(
            OpenCodeEvent::parse("not json"),
            Err(SseError::InvalidEventJson)
        );
        assert_eq!(OpenCodeEvent::parse("{}"), Err(SseError::MissingEventType));
    }
}
