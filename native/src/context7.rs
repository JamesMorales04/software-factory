//! Minimal, bounded Context7 MCP client used by the read-only Librarian.
//!
//! The endpoint is fixed to the public Context7 MCP server. Requests carry no
//! credentials, no repository content beyond the operator's bounded library
//! query, and every response is size- and time-bounded. This is a read-only
//! knowledge source; it cannot call tools in the Factory, write files or grant
//! authority.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::knowledge::KnowledgeSource;

pub const CONTEXT7_ENDPOINT: &str = "https://mcp.context7.com/mcp";
pub const MCP_PROTOCOL_VERSION: &str = "2025-03-26";
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_SNIPPET_BYTES: usize = 16 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(25);

#[derive(Debug, thiserror::Error)]
pub enum Context7Error {
    #[error("Context7 MCP configuration is invalid: {0}")]
    Invalid(String),
    #[error("Context7 MCP request is unavailable: {0}")]
    Unavailable(String),
    #[error("Context7 MCP returned no usable result: {0}")]
    Empty(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLibrary {
    pub title: String,
    pub library_id: String,
    pub description: String,
    pub code_snippets: u64,
    pub reputation: Option<String>,
    pub benchmark_score: Option<f64>,
    pub versions: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Context7Client {
    client: reqwest::Client,
    endpoint: String,
}

struct Session {
    id: Option<String>,
    next_id: u64,
}

impl Context7Client {
    pub fn production() -> Result<Self, Context7Error> {
        Self::with_endpoint(CONTEXT7_ENDPOINT, false)
    }

    pub fn with_endpoint(endpoint: &str, allow_loopback_http: bool) -> Result<Self, Context7Error> {
        let url = reqwest::Url::parse(endpoint)
            .map_err(|_| Context7Error::Invalid("Context7 endpoint is not a URL".into()))?;
        let https = url.scheme() == "https";
        let loopback_http = allow_loopback_http
            && url.scheme() == "http"
            && url
                .host_str()
                .is_some_and(|host| matches!(host, "127.0.0.1" | "::1" | "localhost"));
        if !https && !loopback_http {
            return Err(Context7Error::Invalid(
                "Context7 endpoint must be HTTPS (loopback HTTP is test-only)".into(),
            ));
        }
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| Context7Error::Invalid(error.to_string()))?;
        Ok(Self {
            client,
            endpoint: endpoint.to_owned(),
        })
    }

    async fn post(
        &self,
        body: &Value,
        session: Option<&str>,
    ) -> Result<(Value, Option<String>), Context7Error> {
        let mut request = self
            .client
            .post(&self.endpoint)
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION)
            .json(body);
        if let Some(session) = session {
            request = request.header("Mcp-Session-Id", session);
        }
        let response = request
            .send()
            .await
            .map_err(|error| Context7Error::Unavailable(error.to_string()))?;
        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let session_id = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let bytes = response
            .bytes()
            .await
            .map_err(|error| Context7Error::Unavailable(error.to_string()))?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(Context7Error::Invalid(
                "Context7 response exceeds the bounded size".into(),
            ));
        }
        if !status.is_success() {
            return Err(Context7Error::Unavailable(format!(
                "Context7 returned HTTP {status}"
            )));
        }
        if bytes.is_empty() {
            // JSON-RPC notifications may return 202 with no body.
            return Ok((Value::Null, session_id));
        }
        let value = if content_type.contains("text/event-stream") {
            parse_sse(&bytes)?
        } else {
            serde_json::from_slice(&bytes)
                .map_err(|error| Context7Error::Unavailable(error.to_string()))?
        };
        Ok((value, session_id))
    }

    async fn open(&self) -> Result<Session, Context7Error> {
        let (response, session) = self
            .post(
                &json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": MCP_PROTOCOL_VERSION,
                        "capabilities": {},
                        "clientInfo": {"name": "software-factory-librarian", "version": "0.1.0"}
                    }
                }),
                None,
            )
            .await?;
        let result = response
            .get("result")
            .ok_or_else(|| Context7Error::Empty("Context7 initialize returned no result".into()))?;
        if result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .is_none()
        {
            return Err(Context7Error::Empty(
                "Context7 initialize returned no protocol version".into(),
            ));
        }
        let session = Session {
            id: session,
            next_id: 2,
        };
        let (notification, _) = self
            .post(
                &json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/initialized",
                    "params": {}
                }),
                session.id.as_deref(),
            )
            .await?;
        if !notification.is_null() {
            return Err(Context7Error::Empty(
                "Context7 initialized notification returned an unexpected result".into(),
            ));
        }
        Ok(session)
    }

    async fn call_tool(
        &self,
        session: &mut Session,
        name: &str,
        arguments: Value,
    ) -> Result<String, Context7Error> {
        let id = session.next_id;
        session.next_id += 1;
        let (response, _) = self
            .post(
                &json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "tools/call",
                    "params": {"name": name, "arguments": arguments}
                }),
                session.id.as_deref(),
            )
            .await?;
        if let Some(error) = response.get("error") {
            return Err(Context7Error::Unavailable(format!(
                "Context7 tool {name} failed: {error}"
            )));
        }
        let result = response.get("result").ok_or_else(|| {
            Context7Error::Empty(format!("Context7 tool {name} returned no result"))
        })?;
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            return Err(Context7Error::Unavailable(format!(
                "Context7 tool {name} reported an error"
            )));
        }
        let text = result
            .get("content")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|item| item.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        if text.trim().is_empty() {
            return Err(Context7Error::Empty(format!(
                "Context7 tool {name} returned no text"
            )));
        }
        Ok(text)
    }

    /// Resolve a library name and rank the candidates deterministically:
    /// exact title first, then benchmark score, then code-snippet coverage.
    pub async fn resolve_library(
        &self,
        library_name: &str,
        query: &str,
    ) -> Result<Vec<ResolvedLibrary>, Context7Error> {
        if library_name.trim().is_empty()
            || library_name.len() > 100
            || query.trim().is_empty()
            || query.len() > 500
        {
            return Err(Context7Error::Invalid(
                "Context7 library query is outside the bound".into(),
            ));
        }
        let mut session = self.open().await?;
        let text = self
            .call_tool(
                &mut session,
                "resolve-library-id",
                json!({"libraryName": library_name, "query": query}),
            )
            .await?;
        let mut libraries = parse_resolved_libraries(&text);
        libraries.sort_by(|left, right| {
            let exact = right
                .title
                .eq_ignore_ascii_case(library_name)
                .cmp(&left.title.eq_ignore_ascii_case(library_name));
            exact
                .then_with(|| {
                    right
                        .benchmark_score
                        .unwrap_or(0.0)
                        .total_cmp(&left.benchmark_score.unwrap_or(0.0))
                })
                .then_with(|| right.code_snippets.cmp(&left.code_snippets))
                .then_with(|| left.library_id.cmp(&right.library_id))
        });
        if libraries.is_empty() {
            return Err(Context7Error::Empty(
                "Context7 returned no library candidates".into(),
            ));
        }
        Ok(libraries)
    }

    /// Fetch bounded documentation for one resolved library id.
    pub async fn query_docs(&self, library_id: &str, query: &str) -> Result<String, Context7Error> {
        if !library_id.starts_with('/')
            || library_id.len() > 200
            || library_id.chars().any(char::is_control)
            || query.trim().is_empty()
            || query.len() > 500
        {
            return Err(Context7Error::Invalid(
                "Context7 library id or query is invalid".into(),
            ));
        }
        let mut session = self.open().await?;
        let text = self
            .call_tool(
                &mut session,
                "query-docs",
                json!({"libraryId": library_id, "query": query}),
            )
            .await?;
        Ok(bound_text(&text, MAX_SNIPPET_BYTES))
    }

    /// Fetch a KnowledgePack for one library. `resolve_library` is always
    /// called first; the operator cannot bypass it with an arbitrary URL.
    pub async fn knowledge_pack(
        &self,
        library_name: &str,
        query: &str,
        fetched_at: &str,
    ) -> Result<crate::knowledge::KnowledgePack, Context7Error> {
        let candidates = self.resolve_library(library_name, query).await?;
        let chosen = candidates.first().ok_or_else(|| {
            Context7Error::Empty("Context7 returned no library candidates".into())
        })?;
        let docs = self.query_docs(&chosen.library_id, query).await?;
        let pack = crate::knowledge::KnowledgePack {
            library: library_name.to_owned(),
            library_id: chosen.library_id.clone(),
            query: query.to_owned(),
            sources: vec![KnowledgeSource {
                title: format!("{} — Context7", chosen.title),
                url: format!(
                    "https://context7.com/{}",
                    chosen.library_id.trim_start_matches('/')
                ),
                kind: "CONTEXT7_MCP".into(),
                snippet: docs,
                reputation: chosen.reputation.clone(),
                benchmark_score: chosen.benchmark_score,
            }],
            pack_sha256: String::new(),
            fetched_at: fetched_at.to_owned(),
        }
        .seal();
        Ok(pack)
    }
}

fn parse_sse(bytes: &[u8]) -> Result<Value, Context7Error> {
    let text = String::from_utf8_lossy(bytes);
    let mut message = None;
    for line in text.lines() {
        let line = line.trim_start();
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(data)
            && (value.get("id").is_some() || value.get("result").is_some())
        {
            message = Some(value);
        }
    }
    message.ok_or_else(|| Context7Error::Empty("Context7 SSE response had no message".into()))
}

fn bound_text(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[truncated]", &text[..end])
}

fn parse_resolved_libraries(text: &str) -> Vec<ResolvedLibrary> {
    let mut libraries = Vec::new();
    for block in text.split("----------") {
        let mut title = None;
        let mut library_id = None;
        let mut description = String::new();
        let mut code_snippets = 0u64;
        let mut reputation = None;
        let mut benchmark_score = None;
        let mut versions = Vec::new();
        for line in block.lines() {
            let line = line.trim().trim_start_matches('-').trim();
            if let Some(value) = line.strip_prefix("Title:") {
                title = Some(value.trim().to_owned());
            } else if let Some(value) = line.strip_prefix("Context7-compatible library ID:") {
                library_id = Some(value.trim().to_owned());
            } else if let Some(value) = line.strip_prefix("Description:") {
                description = value.trim().to_owned();
            } else if let Some(value) = line.strip_prefix("Code Snippets:") {
                code_snippets = value.trim().parse().unwrap_or(0);
            } else if let Some(value) = line.strip_prefix("Source Reputation:") {
                reputation = Some(value.trim().to_owned());
            } else if let Some(value) = line.strip_prefix("Benchmark Score:") {
                benchmark_score = value.trim().parse().ok();
            } else if let Some(value) = line.strip_prefix("Versions:") {
                versions = value
                    .split(',')
                    .map(|version| version.trim().to_owned())
                    .filter(|version| !version.is_empty())
                    .collect();
            }
        }
        if let (Some(title), Some(library_id)) = (title, library_id)
            && library_id.starts_with('/')
            && library_id.len() <= 200
        {
            libraries.push(ResolvedLibrary {
                title,
                library_id,
                description,
                code_snippets,
                reputation,
                benchmark_score,
                versions,
            });
        }
    }
    libraries
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, extract::State, http::HeaderMap, response::IntoResponse, routing::post};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Fake {
        calls: Arc<Mutex<Vec<Value>>>,
        session_headers: Arc<Mutex<Vec<Option<String>>>>,
    }

    async fn handler(
        State(fake): State<Fake>,
        headers: HeaderMap,
        body: String,
    ) -> impl IntoResponse {
        let value: Value = serde_json::from_str(&body).unwrap();
        fake.calls.lock().unwrap().push(value.clone());
        fake.session_headers.lock().unwrap().push(
            headers
                .get("mcp-session-id")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
        );
        let method = value.get("method").and_then(Value::as_str).unwrap_or("");
        if method == "notifications/initialized" {
            return (
                axum::http::StatusCode::ACCEPTED,
                [("content-type", "application/json")],
                String::new(),
            )
                .into_response();
        }
        let id = value.get("id").cloned().unwrap_or(Value::Null);
        let result = if method == "initialize" {
            json!({"protocolVersion":"2025-03-26","serverInfo":{"name":"Context7","version":"test"}})
        } else {
            let name = value
                .pointer("/params/name")
                .and_then(Value::as_str)
                .unwrap_or("");
            if name == "resolve-library-id" {
                json!({"content":[{"type":"text","text":"Available Libraries:\n\n- Title: Chrono\n- Context7-compatible library ID: /chronotope/chrono\n- Description: Dates\n- Code Snippets: 100\n- Source Reputation: High\n- Benchmark Score: 90\n----------\n- Title: Other\n- Context7-compatible library ID: /other/other\n- Description: Other\n- Code Snippets: 10\n"}]})
            } else {
                json!({"content":[{"type":"text","text":"Use Utc::now().to_rfc3339()."}]})
            }
        };
        let message = json!({"jsonrpc":"2.0","id":id,"result":result});
        (
            axum::http::StatusCode::OK,
            [
                ("content-type", "text/event-stream"),
                ("mcp-session-id", "test-session"),
            ],
            format!("event: message\ndata: {message}\n\n"),
        )
            .into_response()
    }

    async fn server() -> (String, Fake) {
        let fake = Fake::default();
        let app = Router::new()
            .route("/mcp", post(handler))
            .with_state(fake.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}/mcp"), fake)
    }

    #[tokio::test]
    async fn resolves_ranks_and_fetches_bounded_docs() {
        let (endpoint, fake) = server().await;
        let client = Context7Client::with_endpoint(&endpoint, true).unwrap();
        let candidates = client.resolve_library("chrono", "dates").await.unwrap();
        assert_eq!(candidates[0].library_id, "/chronotope/chrono");
        assert_eq!(candidates[0].benchmark_score, Some(90.0));
        let pack = client
            .knowledge_pack("chrono", "dates", "2026-11-09T00:00:00Z")
            .await
            .unwrap();
        assert_eq!(pack.library_id, "/chronotope/chrono");
        assert!(pack.sources[0].snippet.contains("Utc::now"));
        let calls = fake.calls.lock().unwrap().clone();
        assert_eq!(
            calls
                .iter()
                .filter(|call| call.get("method").and_then(Value::as_str) == Some("initialize"))
                .count(),
            3
        );
        let headers = fake.session_headers.lock().unwrap().clone();
        assert!(
            headers
                .iter()
                .flatten()
                .all(|header| header == "test-session")
        );
    }

    #[tokio::test]
    async fn rejects_non_https_endpoints() {
        assert!(Context7Client::with_endpoint("http://example.com/mcp", false).is_err());
    }

    #[test]
    fn sse_parser_ignores_keepalive_and_selects_message() {
        let value = parse_sse(b": keepalive\n\nevent: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\n").unwrap();
        assert_eq!(value["id"], 1);
    }
}
