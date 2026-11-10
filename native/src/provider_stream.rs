//! Transport-agnostic provider event-stream waiting with bounded reconnect.
//!
//! This module owns the reconnect policy, the SSE replay cursor and duplicate
//! suppression. It deliberately does not know provider terminal semantics and
//! never creates provider sessions, candidates or external effects. A caller
//! supplies the connection transport and a terminal classifier.
//!
//! A successful HTTP reconnect is not success: only a terminal classification
//! returned by the caller ends a wait successfully. A stream cut before that
//! terminal is retried up to [`SseReconnectPolicy::max_attempts`] and then
//! fails closed.

use std::{future::Future, pin::Pin, time::Duration};

use bytes::Bytes;
use futures_util::{Stream, StreamExt};

use crate::provider_reliability::{SseCursor, SseReconnectPolicy};
use crate::sse::{SseEvent, SseParser};

/// Maximum bytes for one SSE record and its accumulated fields.
pub const MAX_EVENT_RECORD_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("provider stream transport failed: {message}")]
pub struct StreamTransportError {
    pub message: String,
}

impl StreamTransportError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

pub type EventChunk = Result<Bytes, StreamTransportError>;

/// One live provider event stream. Reconnects replace this value; the waiter
/// never assumes that a replacement stream resumes correctly on its own.
pub type BoxEventStream = Pin<Box<dyn Stream<Item = EventChunk> + Send>>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderStreamError {
    #[error("provider event stream connection failed: {0}")]
    Connect(String),
    #[error("provider event stream protocol error: {0}")]
    Protocol(String),
    #[error("provider SSE reconnect attempts exhausted after {attempts}")]
    ReconnectExhausted { attempts: u32 },
}

/// Opens provider event streams. The implementation must forward
/// `last_event_id` as the provider's SSE replay cursor, for example the
/// `Last-Event-ID` request header.
pub trait EventStreamConnector: Send {
    fn connect(
        &mut self,
        last_event_id: Option<&str>,
    ) -> impl Future<Output = Result<BoxEventStream, ProviderStreamError>> + Send;
}

/// Injectable sleep source. Production uses Tokio timers; tests use a
/// recording or virtual clock and never sleep for the real deadline.
pub trait WaitClock: Send + Sync {
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl WaitClock for SystemClock {
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(tokio::time::sleep(duration))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalDecision<T> {
    Continue,
    Terminal(T),
    Fatal(ProviderStreamError),
}

/// Waits for one terminal event on one provider turn.
///
/// The waiter retains the last complete event id and reconnects with it after a
/// cut. Duplicate event ids accepted from a replayed stream are skipped before
/// the classifier sees them, so an already-applied effect is not repeated.
pub struct ReconnectWaiter<C> {
    connector: C,
    policy: SseReconnectPolicy,
    cursor: SseCursor,
    stream: Option<BoxEventStream>,
    max_record_bytes: usize,
}

impl<C: EventStreamConnector> ReconnectWaiter<C> {
    pub fn new(connector: C, policy: SseReconnectPolicy) -> Self {
        Self {
            connector,
            policy,
            cursor: SseCursor::new(),
            stream: None,
            max_record_bytes: MAX_EVENT_RECORD_BYTES,
        }
    }

    /// Reuse an already-open stream for the first wait attempt. Reconnects use
    /// the connector.
    pub fn with_stream(mut self, stream: BoxEventStream) -> Self {
        self.stream = Some(stream);
        self
    }

    pub fn cursor(&self) -> &SseCursor {
        &self.cursor
    }

    pub async fn wait<H, T>(
        &mut self,
        clock: &dyn WaitClock,
        mut classify: H,
    ) -> Result<T, ProviderStreamError>
    where
        H: FnMut(&SseEvent) -> TerminalDecision<T>,
    {
        self.policy
            .validate()
            .map_err(ProviderStreamError::Protocol)?;
        // The reconnect bound is per turn/wait. The replay cursor is retained
        // across turns so a later reconnect still resumes from the last event.
        self.cursor.reset_reconnects();
        let mut stream = match self.stream.take() {
            Some(stream) => stream,
            None => {
                self.connector
                    .connect(self.cursor.last_event_id.as_deref())
                    .await?
            }
        };
        let mut parser = SseParser::new(self.max_record_bytes);
        let result = 'wait: loop {
            match stream.next().await {
                Some(Ok(chunk)) => {
                    let frames = match parser.push(&chunk) {
                        Ok(frames) => frames,
                        Err(error) => {
                            break 'wait Err(ProviderStreamError::Protocol(error.to_string()));
                        }
                    };
                    for frame in frames {
                        if let Some(id) = frame.id.as_deref() {
                            match self.cursor.observe(Some(id)) {
                                Ok(true) => {}
                                Ok(false) => continue,
                                Err(error) => {
                                    break 'wait Err(ProviderStreamError::Protocol(error));
                                }
                            }
                        }
                        match classify(&frame) {
                            TerminalDecision::Continue => {}
                            TerminalDecision::Terminal(value) => break 'wait Ok(value),
                            TerminalDecision::Fatal(error) => break 'wait Err(error),
                        }
                    }
                    continue 'wait;
                }
                Some(Err(error)) => {
                    tracing::debug!(%error, "provider event stream cut; reconnecting");
                }
                None => {
                    tracing::debug!("provider event stream ended before terminal evidence");
                }
            }
            let delay = self.cursor.reconnect(&self.policy).map_err(|_| {
                ProviderStreamError::ReconnectExhausted {
                    attempts: self.cursor.reconnects,
                }
            })?;
            clock.sleep(delay).await;
            parser = SseParser::new(self.max_record_bytes);
            stream = self
                .connector
                .connect(self.cursor.last_event_id.as_deref())
                .await?;
        };
        self.stream = Some(stream);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    struct IterConnector {
        scripts: VecDeque<Vec<EventChunk>>,
        cursors: Arc<Mutex<Vec<Option<String>>>>,
    }

    impl EventStreamConnector for IterConnector {
        fn connect(
            &mut self,
            last_event_id: Option<&str>,
        ) -> impl Future<Output = Result<BoxEventStream, ProviderStreamError>> + Send {
            self.cursors
                .lock()
                .expect("cursor log")
                .push(last_event_id.map(str::to_owned));
            let chunks = self.scripts.pop_front().unwrap_or_default();
            async move { Ok(Box::pin(futures_util::stream::iter(chunks)) as BoxEventStream) }
        }
    }

    struct ZeroClock;

    impl WaitClock for ZeroClock {
        fn sleep(&self, _duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            Box::pin(std::future::ready(()))
        }
    }

    fn event(id: &str, kind: &str) -> String {
        format!(
            "id: {id}\ndata: {{\"type\":\"{kind}\",\"properties\":{{\"sessionID\":\"ses_1\"}}}}\n\n"
        )
    }

    #[tokio::test]
    async fn terminal_event_completes_and_duplicate_ids_are_skipped() {
        let cursors = Arc::new(Mutex::new(Vec::new()));
        let connector = IterConnector {
            scripts: VecDeque::from([vec![
                Ok(Bytes::from(event("e1", "message.updated"))),
                Ok(Bytes::from(event("e1", "message.updated"))),
                Ok(Bytes::from(event("e2", "session.idle"))),
            ]]),
            cursors: cursors.clone(),
        };
        let mut waiter = ReconnectWaiter::new(
            connector,
            SseReconnectPolicy {
                max_attempts: 2,
                base_delay_ms: 1,
                max_delay_ms: 2,
            },
        );
        let mut effects = 0;
        let terminal = waiter
            .wait(&ZeroClock, |frame| {
                if frame.data.contains("session.idle") {
                    TerminalDecision::Terminal("idle")
                } else {
                    effects += 1;
                    TerminalDecision::Continue
                }
            })
            .await
            .unwrap();
        assert_eq!(terminal, "idle");
        assert_eq!(effects, 1);
        assert_eq!(*cursors.lock().unwrap(), vec![None]);
    }

    #[tokio::test]
    async fn missing_terminal_exhausts_reconnect_budget() {
        let cursors = Arc::new(Mutex::new(Vec::new()));
        let connector = IterConnector {
            scripts: VecDeque::from([
                vec![Ok(Bytes::from(event("e1", "message.updated")))],
                vec![Ok(Bytes::from(event("e1", "message.updated")))],
                vec![Ok(Bytes::from(event("e1", "message.updated")))],
            ]),
            cursors: cursors.clone(),
        };
        let mut waiter = ReconnectWaiter::new(
            connector,
            SseReconnectPolicy {
                max_attempts: 2,
                base_delay_ms: 1,
                max_delay_ms: 2,
            },
        );
        let error = waiter
            .wait(&ZeroClock, |_| TerminalDecision::<()>::Continue)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ProviderStreamError::ReconnectExhausted { attempts: 2 }
        ));
        assert_eq!(
            *cursors.lock().unwrap(),
            vec![None, Some("e1".into()), Some("e1".into())]
        );
    }
}
