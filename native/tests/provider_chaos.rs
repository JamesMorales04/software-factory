//! Deterministic provider stream, structured-output and reservation chaos.
//!
//! Every fixture in this file is local and offline: scripted in-memory streams
//! or a loopback-only HTTP server bound to `127.0.0.1`. No test resolves or
//! contacts an external provider. The suite exercises the same reconnect
//! waiter and OpenCode SSE classifier used by the Worker.

use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, HeaderValue, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
    routing::get,
};
use bytes::Bytes;
use factory_native::{
    provider_patch::{ProviderAction, extract_action_text},
    provider_reliability::{
        MAX_STRUCTURED_REPAIRS, SseReconnectPolicy, StructuredOutputError, StructuredOutputRepair,
        TurnReservation, decode_structured_output,
    },
    provider_session::{
        PermissionDecision, ProviderEvent, ProviderKind, ProviderSession, SessionError,
        SessionState, TerminalStatus,
    },
    provider_stream::{
        BoxEventStream, EventChunk, EventStreamConnector, ProviderStreamError, ReconnectWaiter,
        StreamTransportError, TerminalDecision, WaitClock,
    },
    runtime::{OpenCodeEventConnector, OpenCodePromptTerminal, classify_opencode_terminal_event},
    sse::{OpenCodeEvent, SseEvent},
};
use futures_util::Stream;
use serde_json::json;

const SESSION: &str = "ses_chaos";

#[derive(Debug, Default)]
struct Observations {
    effects: u64,
    candidates: u64,
}

fn frame(id: &str, kind: &str, session: &str) -> String {
    format!(
        "id: {id}\ndata: {}\n\n",
        json!({"type": kind, "properties": {"sessionID": session}})
    )
}

fn provider_error(id: &str, session: &str) -> String {
    format!(
        "id: {id}\ndata: {}\n\n",
        json!({
            "type": "session.error",
            "properties": {"sessionID": session, "error": {"name": "ProviderError"}}
        })
    )
}

fn chunk(body: String) -> EventChunk {
    Ok(Bytes::from(body))
}

fn classify(
    event: &SseEvent,
    observations: &mut Observations,
) -> TerminalDecision<OpenCodePromptTerminal> {
    match classify_opencode_terminal_event(event, SESSION, false) {
        Ok(Some(
            terminal @ OpenCodePromptTerminal::Idle {
                cancel_requested: false,
                ..
            },
        )) => {
            observations.candidates = observations.candidates.saturating_add(1);
            TerminalDecision::Terminal(terminal)
        }
        Ok(Some(terminal)) => TerminalDecision::Terminal(terminal),
        Ok(None) => {
            match OpenCodeEvent::parse(&event.data) {
                Ok(parsed) if parsed.kind.starts_with("message.") => {
                    observations.effects = observations.effects.saturating_add(1);
                }
                Ok(_) => {}
                Err(error) => {
                    return TerminalDecision::Fatal(ProviderStreamError::Protocol(
                        error.to_string(),
                    ));
                }
            }
            TerminalDecision::Continue
        }
        Err(error) => TerminalDecision::Fatal(ProviderStreamError::Protocol(error.to_string())),
    }
}

struct ScriptedConnector {
    scripts: VecDeque<Vec<EventChunk>>,
    cursors: Arc<Mutex<Vec<Option<String>>>>,
}

impl ScriptedConnector {
    fn new(scripts: Vec<Vec<EventChunk>>) -> (Self, Arc<Mutex<Vec<Option<String>>>>) {
        let cursors = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                scripts: scripts.into(),
                cursors: cursors.clone(),
            },
            cursors,
        )
    }
}

impl EventStreamConnector for ScriptedConnector {
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

#[derive(Debug, Default)]
struct VirtualClock {
    elapsed_ms: AtomicU64,
    sleeps: Mutex<Vec<Duration>>,
}

impl VirtualClock {
    fn advance(&self, millis: u64) {
        self.elapsed_ms.fetch_add(millis, Ordering::SeqCst);
    }

    fn elapsed(&self) -> Duration {
        Duration::from_millis(self.elapsed_ms.load(Ordering::SeqCst))
    }
}

impl WaitClock for VirtualClock {
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        self.sleeps.lock().expect("sleep log").push(duration);
        self.advance(u64::try_from(duration.as_millis()).unwrap_or(u64::MAX));
        Box::pin(std::future::ready(()))
    }
}

fn policy(max_attempts: u32) -> SseReconnectPolicy {
    SseReconnectPolicy {
        max_attempts,
        base_delay_ms: 1,
        max_delay_ms: 4,
    }
}

#[tokio::test]
async fn sse_cut_reconnects_with_last_event_id_and_does_not_repeat_effects() {
    let (connector, cursors) = ScriptedConnector::new(vec![
        vec![
            chunk(frame("e1", "message.updated", SESSION)),
            chunk(frame("e2", "message.part.updated", SESSION)),
            Err(StreamTransportError::new("fixture cut mid-turn")),
        ],
        vec![
            chunk(frame("e2", "message.part.updated", SESSION)),
            chunk(frame("e3", "session.idle", SESSION)),
        ],
    ]);
    let mut waiter = ReconnectWaiter::new(connector, policy(4));
    let clock = VirtualClock::default();
    let mut observations = Observations::default();
    let terminal = waiter
        .wait(&clock, |event| classify(event, &mut observations))
        .await
        .expect("reconnect must reach the terminal event");
    assert!(matches!(
        terminal,
        OpenCodePromptTerminal::Idle {
            cancel_requested: false,
            ..
        }
    ));
    assert_eq!(observations.effects, 2, "replayed e2 must not repeat work");
    assert_eq!(observations.candidates, 1);
    assert_eq!(
        *cursors.lock().unwrap(),
        vec![None, Some("e2".into())],
        "reconnect must carry the last complete event id"
    );
    assert_eq!(
        clock.sleeps.lock().unwrap().as_slice(),
        [Duration::from_millis(1)],
        "one bounded backoff between the two connections"
    );
}

#[tokio::test]
async fn repeated_cuts_exhaust_the_reconnect_bound_and_fail_closed() {
    let scripts = (0..4)
        .map(|_| {
            vec![
                chunk(frame("e1", "message.updated", SESSION)),
                Err(StreamTransportError::new("fixture cut")),
            ]
        })
        .collect();
    let (connector, cursors) = ScriptedConnector::new(scripts);
    let mut waiter = ReconnectWaiter::new(connector, policy(3));
    let mut observations = Observations::default();
    let error = waiter
        .wait(&VirtualClock::default(), |event| {
            classify(event, &mut observations)
        })
        .await
        .expect_err("every connection cuts before the terminal event");
    assert_eq!(
        error,
        ProviderStreamError::ReconnectExhausted { attempts: 3 }
    );
    assert_eq!(cursors.lock().unwrap().len(), 4, "initial + three retries");
    assert_eq!(observations.candidates, 0);
    assert_eq!(observations.effects, 1, "duplicate replay is suppressed");
}

#[tokio::test]
async fn long_turn_stays_live_beyond_the_former_three_hundred_second_deadline() {
    struct TickingConnector {
        clock: Arc<VirtualClock>,
    }

    struct TickingStream {
        remaining: usize,
        terminal_sent: bool,
        clock: Arc<VirtualClock>,
    }

    impl Stream for TickingStream {
        type Item = EventChunk;

        fn poll_next(
            mut self: Pin<&mut Self>,
            _context: &mut Context<'_>,
        ) -> Poll<Option<Self::Item>> {
            let this = self.as_mut().get_mut();
            if this.remaining > 0 {
                this.remaining -= 1;
                this.clock.advance(1_000);
                return Poll::Ready(Some(chunk(frame(
                    &format!("h{}", this.remaining),
                    "message.part.updated",
                    SESSION,
                ))));
            }
            if !this.terminal_sent {
                this.terminal_sent = true;
                return Poll::Ready(Some(chunk(frame("done", "session.idle", SESSION))));
            }
            Poll::Ready(None)
        }
    }

    impl EventStreamConnector for TickingConnector {
        fn connect(
            &mut self,
            _last_event_id: Option<&str>,
        ) -> impl Future<Output = Result<BoxEventStream, ProviderStreamError>> + Send {
            let clock = self.clock.clone();
            async move {
                Ok(Box::pin(TickingStream {
                    // 301 provider-observed seconds, without any real sleep.
                    remaining: 301,
                    terminal_sent: false,
                    clock,
                }) as BoxEventStream)
            }
        }
    }

    let clock = Arc::new(VirtualClock::default());
    let mut waiter = ReconnectWaiter::new(
        TickingConnector {
            clock: clock.clone(),
        },
        policy(2),
    );
    let mut observations = Observations::default();
    let terminal = waiter
        .wait(clock.as_ref(), |event| classify(event, &mut observations))
        .await
        .expect("a long healthy turn must not hit a hidden deadline");
    assert!(matches!(terminal, OpenCodePromptTerminal::Idle { .. }));
    assert!(
        clock.elapsed() > Duration::from_secs(300),
        "fixture advanced {:?} of virtual provider time",
        clock.elapsed()
    );
    assert_eq!(observations.effects, 301);
    assert_eq!(observations.candidates, 1);
}

#[tokio::test]
async fn duplicate_event_ids_do_not_repeat_effects() {
    let (connector, _) = ScriptedConnector::new(vec![vec![
        chunk(frame("e1", "message.updated", SESSION)),
        chunk(frame("e1", "message.updated", SESSION)),
        chunk(frame("e2", "session.idle", SESSION)),
    ]]);
    let mut waiter = ReconnectWaiter::new(connector, policy(2));
    let mut observations = Observations::default();
    waiter
        .wait(&VirtualClock::default(), |event| {
            classify(event, &mut observations)
        })
        .await
        .unwrap();
    assert_eq!(observations.effects, 1);
    assert_eq!(observations.candidates, 1);
}

#[tokio::test]
async fn missing_terminal_event_returns_no_candidate() {
    let (connector, cursors) = ScriptedConnector::new(vec![
        vec![
            chunk(frame("e1", "message.part.updated", SESSION)),
            Err(StreamTransportError::new("stream ends before terminal")),
        ],
        vec![
            chunk(frame("e1", "message.part.updated", SESSION)),
            Err(StreamTransportError::new("stream ends before terminal")),
        ],
    ]);
    let mut waiter = ReconnectWaiter::new(connector, policy(1));
    let mut observations = Observations::default();
    let error = waiter
        .wait(&VirtualClock::default(), |event| {
            classify(event, &mut observations)
        })
        .await
        .expect_err("a cut stream is not completion evidence");
    assert_eq!(
        error,
        ProviderStreamError::ReconnectExhausted { attempts: 1 }
    );
    assert_eq!(observations.candidates, 0, "no terminal means no candidate");
    assert_eq!(observations.effects, 1);
    assert_eq!(*cursors.lock().unwrap(), vec![None, Some("e1".into())]);
}

#[tokio::test]
async fn provider_error_terminal_is_not_a_completed_candidate() {
    let (connector, _) = ScriptedConnector::new(vec![vec![
        chunk(frame("e1", "message.partial.updated", SESSION)),
        chunk(provider_error("e2", SESSION)),
    ]]);
    let mut waiter = ReconnectWaiter::new(connector, policy(2));
    let mut observations = Observations::default();
    let terminal = waiter
        .wait(&VirtualClock::default(), |event| {
            classify(event, &mut observations)
        })
        .await
        .unwrap();
    assert!(matches!(terminal, OpenCodePromptTerminal::Failed { .. }));
    assert_eq!(observations.effects, 1);
    assert_eq!(
        observations.candidates, 0,
        "a provider error terminal never creates a candidate"
    );
}

#[test]
fn malformed_structured_json_is_repaired_then_accepted() {
    let mut repair = StructuredOutputRepair::new(MAX_STRUCTURED_REPAIRS).unwrap();
    let rejected = decode_structured_output(&mut repair, || {
        extract_action_text("{\"type\":\"final\",\"files\":[").map_err(|error| error.to_string())
    })
    .expect_err("truncated JSON must not become an action");
    assert!(matches!(
        rejected,
        StructuredOutputError::Repair {
            attempt: 1,
            max_repairs: MAX_STRUCTURED_REPAIRS,
            ..
        }
    ));
    let action = decode_structured_output(&mut repair, || {
        extract_action_text("{\"type\":\"final\",\"files\":[],\"explanation\":\"repaired\"}")
            .map_err(|error| error.to_string())
    })
    .expect("the repaired object is valid");
    assert!(matches!(action, ProviderAction::Final { .. }));
    assert_eq!(repair.attempts(), 0);
}

#[test]
fn structured_json_repair_exhaustion_is_a_typed_failure() {
    let mut repair = StructuredOutputRepair::new(2).unwrap();
    for attempt in 1..=2 {
        let error = decode_structured_output(&mut repair, || {
            extract_action_text("{\"type\":\"final\",\"files\":[")
                .map_err(|parse| parse.to_string())
        })
        .expect_err("malformed JSON must fail");
        assert!(matches!(
            error,
            StructuredOutputError::Repair {
                attempt: observed,
                ..
            } if observed == attempt
        ));
    }
    let exhausted = decode_structured_output(&mut repair, || {
        extract_action_text("{\"type\":\"final\",\"files\":[").map_err(|parse| parse.to_string())
    })
    .expect_err("budget is exhausted");
    assert!(matches!(
        exhausted,
        StructuredOutputError::Exhausted { attempts: 2, .. }
    ));
}

#[test]
fn failed_turn_releases_or_settles_a_reservation_exactly_once() {
    let mut released = TurnReservation::reserved();
    released.release().unwrap();
    assert!(released.is_terminal());
    assert!(released.release().is_err());
    assert!(released.settle().is_err());

    let mut settled = TurnReservation::reserved();
    settled.settle().unwrap();
    assert!(settled.is_terminal());
    assert!(settled.settle().is_err());
    assert!(settled.release().is_err());
}

#[test]
fn provider_session_reconnect_and_cleanup_are_exactly_once() {
    let mut session =
        ProviderSession::new(ProviderKind::OpenCode, "deepseek-v4.1-flash", "build").unwrap();
    session
        .apply(&ProviderEvent::SessionStarted {
            sequence: 1,
            session_id: SESSION.into(),
        })
        .unwrap();
    session
        .apply(&ProviderEvent::TurnStarted {
            sequence: 2,
            turn_id: "turn-1".into(),
        })
        .unwrap();
    // A cut before terminal evidence keeps the same session identity; reconnect
    // never creates a second session.
    session.reconnect(SESSION).unwrap();
    assert_eq!(
        session.reconnect("ses_other"),
        Err(SessionError::IdentityMismatch)
    );
    session
        .apply(&ProviderEvent::TurnTerminal {
            sequence: 3,
            turn_id: "turn-1".into(),
            status: TerminalStatus::Completed,
        })
        .unwrap();
    assert_eq!(session.state, SessionState::CleanupPending);
    // A replayed terminal event cannot drive a second cleanup effect.
    assert_eq!(
        session.apply(&ProviderEvent::TurnTerminal {
            sequence: 3,
            turn_id: "turn-1".into(),
            status: TerminalStatus::Completed,
        }),
        Ok(()),
        "identical replay is idempotent"
    );
    assert_eq!(
        session.apply(&ProviderEvent::TurnTerminal {
            sequence: 4,
            turn_id: "turn-1".into(),
            status: TerminalStatus::Completed,
        }),
        Err(SessionError::Terminal)
    );
    assert_eq!(
        session.confirm_cleanup(""),
        Err(SessionError::CleanupUnconfirmed)
    );
    session
        .confirm_cleanup("session_delete_verified_absent")
        .unwrap();
    assert_eq!(session.state, SessionState::Cleaned);
    assert_eq!(
        session.confirm_cleanup("second cleanup"),
        Err(SessionError::CleanupUnconfirmed)
    );
    assert_eq!(
        session.decide_permission("call", PermissionDecision::Allow),
        Err(SessionError::NoPermission)
    );
}

#[tokio::test]
async fn production_connector_forwards_last_event_id_to_a_local_fixture() {
    #[derive(Clone)]
    struct FixtureState {
        cursors: Arc<Mutex<Vec<Option<String>>>>,
    }

    async fn event_stream(State(state): State<FixtureState>, headers: HeaderMap) -> Response {
        let cursor = headers
            .get("last-event-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        state.cursors.lock().unwrap().push(cursor.clone());
        let body = if cursor.as_deref() == Some("e2") {
            format!(
                "{}{}",
                frame("e2", "message.part.updated", SESSION),
                frame("e3", "session.idle", SESSION)
            )
        } else {
            format!(
                "{}{}",
                frame("e1", "message.updated", SESSION),
                frame("e2", "message.part.updated", SESSION)
            )
        };
        let mut response = body.into_response();
        response
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
        response
    }

    let cursors = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/event", get(event_stream))
        .with_state(FixtureState {
            cursors: cursors.clone(),
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let connector =
        OpenCodeEventConnector::new(client, &format!("http://{address}"), "password", "/session")
            .unwrap();
    let mut waiter = ReconnectWaiter::new(connector, policy(3));
    let mut observations = Observations::default();
    let terminal = waiter
        .wait(&VirtualClock::default(), |event| {
            classify(event, &mut observations)
        })
        .await
        .unwrap();
    assert!(matches!(terminal, OpenCodePromptTerminal::Idle { .. }));
    assert_eq!(observations.effects, 2);
    assert_eq!(observations.candidates, 1);
    assert_eq!(
        *cursors.lock().unwrap(),
        vec![None, Some("e2".into())],
        "the production connector must forward the last complete event id"
    );
    server.abort();
}
