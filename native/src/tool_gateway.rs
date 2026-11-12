//! Core-mediated tool gateway for MCP-backed capabilities.
//!
//! The gateway is the only path from an agent role to an MCP server. It owns
//! the curated catalog, role policy, argument validation, rate limits, timeouts
//! and audit records. MCP servers are never connected directly to a Worker,
//! Reviewer or Verifier.
//!
//! This first slice is deliberately local and dependency-light: HTTP MCP
//! (Context7/GitHub read-only) and stdio MCP (Playwright) are supported, but a
//! gateway is disabled unless an explicit configuration is supplied. It does
//! not grant authority, budgets, acceptance or merge rights.

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    process::Stdio,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    time::timeout,
};

const MAX_ARGUMENT_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
/// Hard ceiling for a per-tool call deadline, independent of configuration.
const MAX_TIMEOUT_SECS: u64 = 300;
const MAX_AUDIT_RECORDS: usize = 256;
const DEFAULT_TIMEOUT_SECS: u64 = 30;
const DEFAULT_CALLS_PER_MINUTE: usize = 60;

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("tool gateway is not configured")]
    NotConfigured,
    #[error("unknown tool: {0}")]
    UnknownTool(String),
    #[error("tool {tool} is not allowed for role {role:?}")]
    Denied { role: GatewayRole, tool: String },
    #[error("tool arguments are invalid: {0}")]
    InvalidArguments(String),
    #[error("tool rate limit exceeded for {0}")]
    RateLimited(String),
    #[error("tool transport failed: {0}")]
    Transport(String),
    #[error("tool response exceeded the bound")]
    ResponseTooLarge,
    #[error("tool response is invalid: {0}")]
    InvalidResponse(String),
    #[error("tool configuration is invalid: {0}")]
    Config(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GatewayRole {
    Implementer,
    Repairer,
    Reviewer,
    Verifier,
    Planner,
}

impl GatewayRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Implementer => "IMPLEMENTER",
            Self::Repairer => "REPAIRER",
            Self::Reviewer => "REVIEWER",
            Self::Verifier => "VERIFIER",
            Self::Planner => "PLANNER",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "IMPLEMENTER" => Some(Self::Implementer),
            "REPAIRER" => Some(Self::Repairer),
            "REVIEWER" => Some(Self::Reviewer),
            "VERIFIER" => Some(Self::Verifier),
            "PLANNER" | "LEAD" => Some(Self::Planner),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayToolSpec {
    pub id: String,
    pub server: String,
    pub tool: String,
    pub roles: Vec<GatewayRole>,
    pub read_only: bool,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_max_response")]
    pub max_response_bytes: usize,
}

/// Immutable Core catalog: gateway tool id, MCP server id, exact MCP tool name,
/// read-only classification and a bounded argument hint for the provider prompt.
/// Configuration may select and grant roles for these identities but cannot
/// invent an identity, remap a server, or rename the downstream MCP tool.
#[derive(Debug, Clone, Copy)]
struct KnownTool {
    id: &'static str,
    server: &'static str,
    tool: &'static str,
    read_only: bool,
    hint: &'static str,
}

const KNOWN_TOOLS: &[KnownTool] = &[
    KnownTool {
        id: "context7.resolve-library-id",
        server: "context7",
        tool: "resolve-library-id",
        read_only: true,
        hint: "{\"libraryName\": string, \"query\": string}",
    },
    KnownTool {
        id: "context7.query-docs",
        server: "context7",
        tool: "query-docs",
        read_only: true,
        hint: "{\"libraryId\": \"/org/project\", \"query\": string}",
    },
    KnownTool {
        id: "playwright.browser_snapshot",
        server: "playwright",
        tool: "browser_snapshot",
        read_only: true,
        hint: "{}",
    },
    KnownTool {
        id: "playwright.browser_take_screenshot",
        server: "playwright",
        tool: "browser_take_screenshot",
        read_only: true,
        hint: "{}",
    },
    KnownTool {
        id: "playwright.browser_close",
        server: "playwright",
        tool: "browser_close",
        read_only: false,
        hint: "{}",
    },
    KnownTool {
        id: "github.issue_read",
        server: "github-read",
        tool: "issue_read",
        read_only: true,
        hint: "{\"method\": \"get\"|\"list\", \"owner\": string, \"repo\": string, \"issue_number\": integer}",
    },
    KnownTool {
        id: "github.pull_request_read",
        server: "github-read",
        tool: "pull_request_read",
        read_only: true,
        hint: "{\"method\": \"get\"|\"list\", \"owner\": string, \"repo\": string, \"pullNumber\": integer}",
    },
    KnownTool {
        id: "github.get_file_contents",
        server: "github-read",
        tool: "get_file_contents",
        read_only: true,
        hint: "{\"owner\": string, \"repo\": string, \"path\": string}",
    },
];

fn known_tool(id: &str) -> Option<&'static KnownTool> {
    KNOWN_TOOLS.iter().find(|known| known.id == id)
}

fn core_tool_is_read_only(tool: &GatewayToolSpec) -> bool {
    known_tool(&tool.id).is_some_and(|known| known.read_only)
}

fn role_allowed(spec: &GatewayToolSpec, role: GatewayRole) -> bool {
    role != GatewayRole::Verifier
        && spec.roles.contains(&role)
        && (role != GatewayRole::Reviewer || core_tool_is_read_only(spec))
}

fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_SECS
}

fn default_max_response() -> usize {
    MAX_RESPONSE_BYTES
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum GatewayServerConfig {
    Http {
        id: String,
        endpoint: String,
        #[serde(default)]
        bearer_env: Option<String>,
        #[serde(default)]
        allow_loopback_http: bool,
    },
    Stdio {
        id: String,
        command: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayConfig {
    pub servers: Vec<GatewayServerConfig>,
    pub tools: Vec<GatewayToolSpec>,
    #[serde(default = "default_rate")]
    pub max_calls_per_minute: usize,
}

fn default_rate() -> usize {
    DEFAULT_CALLS_PER_MINUTE
}

impl GatewayConfig {
    /// A conservative example catalog. Servers are only connected when this
    /// configuration is supplied through `FACTORY_TOOL_GATEWAY_CONFIG`.
    pub fn example() -> Self {
        Self {
            servers: vec![
                GatewayServerConfig::Http {
                    id: "context7".into(),
                    endpoint: "https://mcp.context7.com/mcp".into(),
                    bearer_env: None,
                    allow_loopback_http: false,
                },
                GatewayServerConfig::Http {
                    id: "github-read".into(),
                    endpoint: "https://api.githubcopilot.com/mcp/readonly".into(),
                    bearer_env: Some("GITHUB_MCP_TOKEN".into()),
                    allow_loopback_http: false,
                },
                GatewayServerConfig::Stdio {
                    id: "playwright".into(),
                    command: vec![
                        "npx".into(),
                        "-y".into(),
                        "@playwright/mcp@0.0.84".into(),
                        "--isolated".into(),
                        "--allowed-hosts".into(),
                        "127.0.0.1,localhost,::1".into(),
                    ],
                    env: BTreeMap::new(),
                },
            ],
            tools: vec![
                tool(
                    "context7.resolve-library-id",
                    "context7",
                    "resolve-library-id",
                    &[
                        GatewayRole::Implementer,
                        GatewayRole::Repairer,
                        GatewayRole::Planner,
                        GatewayRole::Reviewer,
                    ],
                    true,
                ),
                tool(
                    "context7.query-docs",
                    "context7",
                    "query-docs",
                    &[
                        GatewayRole::Implementer,
                        GatewayRole::Repairer,
                        GatewayRole::Planner,
                        GatewayRole::Reviewer,
                    ],
                    true,
                ),
                tool(
                    "playwright.browser_snapshot",
                    "playwright",
                    "browser_snapshot",
                    &[
                        GatewayRole::Implementer,
                        GatewayRole::Repairer,
                        GatewayRole::Reviewer,
                    ],
                    true,
                ),
                tool(
                    "playwright.browser_take_screenshot",
                    "playwright",
                    "browser_take_screenshot",
                    &[
                        GatewayRole::Implementer,
                        GatewayRole::Repairer,
                        GatewayRole::Reviewer,
                    ],
                    true,
                ),
                tool(
                    "playwright.browser_close",
                    "playwright",
                    "browser_close",
                    &[GatewayRole::Implementer, GatewayRole::Repairer],
                    false,
                ),
                tool(
                    "github.issue_read",
                    "github-read",
                    "issue_read",
                    &[
                        GatewayRole::Implementer,
                        GatewayRole::Repairer,
                        GatewayRole::Reviewer,
                    ],
                    true,
                ),
                tool(
                    "github.pull_request_read",
                    "github-read",
                    "pull_request_read",
                    &[
                        GatewayRole::Implementer,
                        GatewayRole::Repairer,
                        GatewayRole::Reviewer,
                    ],
                    true,
                ),
                tool(
                    "github.get_file_contents",
                    "github-read",
                    "get_file_contents",
                    &[
                        GatewayRole::Implementer,
                        GatewayRole::Repairer,
                        GatewayRole::Reviewer,
                    ],
                    true,
                ),
            ],
            max_calls_per_minute: DEFAULT_CALLS_PER_MINUTE,
        }
    }

    pub fn validate(&self) -> Result<(), GatewayError> {
        if self.servers.is_empty() || self.tools.is_empty() {
            return Err(GatewayError::Config(
                "gateway requires at least one server and one tool".into(),
            ));
        }
        let mut servers = std::collections::BTreeSet::new();
        for server in &self.servers {
            let id = match server {
                GatewayServerConfig::Http {
                    id,
                    endpoint,
                    allow_loopback_http,
                    ..
                } => {
                    let url = reqwest::Url::parse(endpoint).map_err(|_| {
                        GatewayError::Config(format!("invalid MCP endpoint for {id}"))
                    })?;
                    let loopback = url.scheme() == "http"
                        && url
                            .host_str()
                            .is_some_and(|host| matches!(host, "127.0.0.1" | "::1" | "localhost"));
                    if url.scheme() != "https" && !(loopback && *allow_loopback_http) {
                        return Err(GatewayError::Config(format!(
                            "MCP endpoint for {id} must be HTTPS (loopback HTTP is test-only)"
                        )));
                    }
                    let trusted = match id.as_str() {
                        "context7" => endpoint == "https://mcp.context7.com/mcp" || loopback,
                        "github-read" => {
                            endpoint == "https://api.githubcopilot.com/mcp/readonly" || loopback
                        }
                        _ => false,
                    };
                    if !trusted {
                        return Err(GatewayError::Config(format!(
                            "MCP endpoint for {id} is not a trusted server identity"
                        )));
                    }
                    id
                }
                GatewayServerConfig::Stdio { id, command, .. } => {
                    if command.is_empty() || command[0].trim().is_empty() {
                        return Err(GatewayError::Config(format!(
                            "stdio server {id} has no command"
                        )));
                    }
                    if id != "playwright" {
                        return Err(GatewayError::Config(format!(
                            "stdio server {id} is not in the immutable gateway catalog"
                        )));
                    }
                    id
                }
            };
            if !servers.insert(id) {
                return Err(GatewayError::Config(format!(
                    "duplicate gateway server: {id}"
                )));
            }
        }
        let mut tools = std::collections::BTreeSet::new();
        for tool in &self.tools {
            if !tools.insert(&tool.id) {
                return Err(GatewayError::Config(format!(
                    "duplicate gateway tool: {}",
                    tool.id
                )));
            }
            if !servers.contains(&tool.server) {
                return Err(GatewayError::Config(format!(
                    "tool {} references unknown server {}",
                    tool.id, tool.server
                )));
            }
            let Some(known) = known_tool(&tool.id) else {
                return Err(GatewayError::Config(format!(
                    "tool {} is not in the immutable gateway catalog",
                    tool.id
                )));
            };
            if known.server != tool.server {
                return Err(GatewayError::Config(format!(
                    "tool {} must use server {}, not {}",
                    tool.id, known.server, tool.server
                )));
            }
            if known.tool != tool.tool {
                return Err(GatewayError::Config(format!(
                    "tool {} must dispatch MCP tool {}, not {}",
                    tool.id, known.tool, tool.tool
                )));
            }
            if tool.roles.is_empty() {
                return Err(GatewayError::Config(format!(
                    "tool {} has no roles",
                    tool.id
                )));
            }
            if tool.roles.contains(&GatewayRole::Verifier) {
                return Err(GatewayError::Config(format!(
                    "tool {} cannot grant Verifier access",
                    tool.id
                )));
            }
            if tool.roles.contains(&GatewayRole::Reviewer) && !core_tool_is_read_only(tool) {
                return Err(GatewayError::Config(format!(
                    "tool {} is mutating and cannot grant Reviewer access",
                    tool.id
                )));
            }
            if tool.max_response_bytes == 0 || tool.max_response_bytes > MAX_RESPONSE_BYTES {
                return Err(GatewayError::Config(format!(
                    "tool {} response bound must be between 1 and {MAX_RESPONSE_BYTES} bytes",
                    tool.id
                )));
            }
            if tool.timeout_secs == 0 || tool.timeout_secs > MAX_TIMEOUT_SECS {
                return Err(GatewayError::Config(format!(
                    "tool {} timeout must be between 1 and {MAX_TIMEOUT_SECS} seconds",
                    tool.id
                )));
            }
        }
        Ok(())
    }
}

fn tool(
    id: &str,
    server: &str,
    tool: &str,
    roles: &[GatewayRole],
    read_only: bool,
) -> GatewayToolSpec {
    GatewayToolSpec {
        id: id.into(),
        server: server.into(),
        tool: tool.into(),
        roles: roles.to_vec(),
        read_only,
        timeout_secs: DEFAULT_TIMEOUT_SECS,
        max_response_bytes: MAX_RESPONSE_BYTES,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayAuditRecord {
    pub role: GatewayRole,
    pub tool: String,
    pub args_sha256: String,
    pub decision: String,
    pub duration_ms: u64,
    pub response_bytes: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GatewayToolSummary {
    pub id: String,
    pub server: String,
    pub read_only: bool,
    /// Short argument hint for the provider prompt. It is not a schema and the
    /// gateway still validates the exact MCP call at its boundary.
    pub hint: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayServerSummary {
    pub id: String,
    pub transport: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayStatus {
    pub enabled: bool,
    pub servers: Vec<GatewayServerSummary>,
    pub tools: Vec<GatewayToolSummary>,
    pub max_calls_per_minute: usize,
}

impl GatewayStatus {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            servers: Vec::new(),
            tools: Vec::new(),
            max_calls_per_minute: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerToolGatewayRequest {
    pub worker_id: String,
    pub attempt_id: String,
    pub generation: i64,
    pub tool: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerToolGatewayResponse {
    pub tool: String,
    pub content: Value,
}

#[derive(Debug, Default)]
struct AuditLog {
    records: Mutex<VecDeque<GatewayAuditRecord>>,
}

impl AuditLog {
    fn record(&self, record: GatewayAuditRecord) -> GatewayAuditRecord {
        let mut records = self
            .records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        records.push_back(record.clone());
        while records.len() > MAX_AUDIT_RECORDS {
            records.pop_front();
        }
        record
    }

    pub fn snapshot(&self) -> Vec<GatewayAuditRecord> {
        self.records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .cloned()
            .collect()
    }
}

#[derive(Debug, Default)]
struct RateLimiter {
    buckets: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl RateLimiter {
    fn check(&self, key: &str, limit: usize) -> Result<(), GatewayError> {
        if limit == 0 {
            return Err(GatewayError::RateLimited(key.into()));
        }
        let now = Instant::now();
        let mut buckets = self
            .buckets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let bucket = buckets.entry(key.to_string()).or_default();
        while bucket
            .front()
            .is_some_and(|timestamp| now.duration_since(*timestamp) > Duration::from_secs(60))
        {
            bucket.pop_front();
        }
        if bucket.len() >= limit {
            return Err(GatewayError::RateLimited(key.into()));
        }
        bucket.push_back(now);
        Ok(())
    }
}

enum McpClient {
    Http(HttpMcpClient),
    Stdio(Box<StdioMcpClient>),
}

impl McpClient {
    async fn call_tool(
        &self,
        tool: &str,
        arguments: Value,
        timeout_secs: u64,
        max_response_bytes: usize,
    ) -> Result<Value, GatewayError> {
        match self {
            Self::Http(client) => {
                client
                    .call_tool(tool, arguments, timeout_secs, max_response_bytes)
                    .await
            }
            Self::Stdio(client) => {
                client
                    .call_tool(tool, arguments, timeout_secs, max_response_bytes)
                    .await
            }
        }
    }
}

struct HttpMcpClient {
    client: reqwest::Client,
    endpoint: String,
    auth: Option<String>,
    session: Mutex<Option<String>>,
    next_id: AtomicU64,
}

impl HttpMcpClient {
    fn new(endpoint: String, auth: Option<String>) -> Result<Self, GatewayError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| GatewayError::Config(error.to_string()))?;
        Ok(Self {
            client,
            endpoint,
            auth,
            session: Mutex::new(None),
            next_id: AtomicU64::new(1),
        })
    }

    async fn post(
        &self,
        body: &Value,
        session: Option<&str>,
        expected_id: Option<u64>,
        max_response_bytes: usize,
    ) -> Result<(Value, Option<String>), GatewayError> {
        let max_response_bytes = max_response_bytes.min(MAX_RESPONSE_BYTES);
        let mut request = self
            .client
            .post(&self.endpoint)
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header("MCP-Protocol-Version", "2025-03-26")
            .json(body);
        if let Some(auth) = &self.auth {
            request = request.bearer_auth(auth);
        }
        if let Some(session) = session.filter(|session| !session.is_empty()) {
            request = request.header("Mcp-Session-Id", session);
        }
        let response = request
            .send()
            .await
            .map_err(|error| GatewayError::Transport(error.to_string()))?;
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
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| GatewayError::Transport(error.to_string()))?;
            if bytes.len().saturating_add(chunk.len()) > max_response_bytes {
                return Err(GatewayError::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            return Err(GatewayError::Transport(format!(
                "MCP returned HTTP {status}"
            )));
        }
        if bytes.is_empty() {
            return Ok((Value::Null, session_id));
        }
        let values = if content_type.contains("text/event-stream") {
            parse_sse_messages(&bytes)?
        } else {
            vec![
                serde_json::from_slice(&bytes)
                    .map_err(|error| GatewayError::InvalidResponse(error.to_string()))?,
            ]
        };
        if let Some(expected) = expected_id {
            let value = values
                .into_iter()
                .find(|value| value.get("id").and_then(Value::as_u64) == Some(expected))
                .ok_or_else(|| {
                    GatewayError::InvalidResponse(
                        "MCP response did not contain the expected JSON-RPC id".into(),
                    )
                })?;
            if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
                return Err(GatewayError::InvalidResponse(
                    "MCP response is not JSON-RPC 2.0".into(),
                ));
            }
            Ok((value, session_id))
        } else {
            Ok((values.into_iter().next().unwrap_or(Value::Null), session_id))
        }
    }

    async fn initialize(&self) -> Result<String, GatewayError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (response, session) = self
            .post(
                &json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-03-26",
                        "capabilities": {},
                        "clientInfo": {"name": "software-factory-tool-gateway", "version": "0.1.0"}
                    }
                }),
                None,
                Some(id),
                MAX_RESPONSE_BYTES,
            )
            .await?;
        if response
            .get("result")
            .and_then(|result| result.get("protocolVersion"))
            .is_none()
        {
            return Err(GatewayError::InvalidResponse(
                "MCP initialize returned no protocol version".into(),
            ));
        }
        let session = session.unwrap_or_default();
        {
            *self
                .session
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(session.clone());
        }
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        });
        self.post(&notification, Some(&session), None, MAX_RESPONSE_BYTES)
            .await?;
        Ok(session)
    }

    async fn call_tool(
        &self,
        tool: &str,
        arguments: Value,
        timeout_secs: u64,
        max_response_bytes: usize,
    ) -> Result<Value, GatewayError> {
        let operation = async {
            let session = self
                .session
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            let session = match session {
                Some(session) => session,
                None => self.initialize().await?,
            };
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let body = json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {"name": tool, "arguments": arguments}
            });
            self.post(&body, Some(&session), Some(id), max_response_bytes)
                .await
        };
        let (response, _) = timeout(Duration::from_secs(timeout_secs), operation)
            .await
            .map_err(|_| GatewayError::Transport("MCP call timed out".into()))??;
        let result = response
            .get("result")
            .cloned()
            .ok_or_else(|| GatewayError::InvalidResponse("MCP returned no result".into()))?;
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            return Err(GatewayError::InvalidResponse(
                "MCP tool reported an error".into(),
            ));
        }
        let bytes = serde_json::to_vec(&result)
            .map_err(|error| GatewayError::InvalidResponse(error.to_string()))?;
        if bytes.len() > max_response_bytes {
            return Err(GatewayError::ResponseTooLarge);
        }
        Ok(result)
    }
}

fn parse_sse_messages(bytes: &[u8]) -> Result<Vec<Value>, GatewayError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| GatewayError::InvalidResponse(error.to_string()))?;
    let mut values = Vec::new();
    for chunk in text.split("\n\n") {
        let data = chunk
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim)
            .collect::<Vec<_>>()
            .join("\n");
        if !data.is_empty()
            && let Ok(value) = serde_json::from_str::<Value>(&data)
        {
            values.push(value);
        }
    }
    if values.is_empty() {
        return Err(GatewayError::InvalidResponse(
            "MCP SSE response contained no JSON-RPC message".into(),
        ));
    }
    Ok(values)
}

struct StdioProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    initialized: bool,
}

impl Drop for StdioProcess {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

impl StdioProcess {
    fn spawn(command: &[String], env: &BTreeMap<String, String>) -> Result<Self, GatewayError> {
        let (program, arguments) = command
            .split_first()
            .ok_or_else(|| GatewayError::Config("stdio MCP command is empty".into()))?;
        let mut process = Command::new(program);
        process
            .args(arguments)
            .env_clear()
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = process
            .spawn()
            .map_err(|error| GatewayError::Transport(error.to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| GatewayError::Transport("stdio MCP stdin is unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| GatewayError::Transport("stdio MCP stdout is unavailable".into()))?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            initialized: false,
        })
    }

    async fn send(&mut self, value: &Value) -> Result<(), GatewayError> {
        let line = serde_json::to_string(value)
            .map_err(|error| GatewayError::InvalidArguments(error.to_string()))?;
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|error| GatewayError::Transport(error.to_string()))?;
        self.stdin
            .write_all(b"\n")
            .await
            .map_err(|error| GatewayError::Transport(error.to_string()))?;
        self.stdin
            .flush()
            .await
            .map_err(|error| GatewayError::Transport(error.to_string()))
    }

    async fn read_response(
        &mut self,
        expected_id: u64,
        timeout_secs: u64,
        max_response_bytes: usize,
    ) -> Result<Value, GatewayError> {
        for _ in 0..64 {
            let read = timeout(
                Duration::from_secs(timeout_secs),
                self.read_limited_line(max_response_bytes),
            )
            .await
            .map_err(|_| GatewayError::Transport("stdio MCP read timed out".into()))?;
            let line = read?;
            let value: Value = serde_json::from_str(&line)
                .map_err(|error| GatewayError::InvalidResponse(error.to_string()))?;
            if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
                return Err(GatewayError::InvalidResponse(
                    "stdio MCP response is not JSON-RPC 2.0".into(),
                ));
            }
            match value.get("id").and_then(Value::as_u64) {
                Some(id) if id == expected_id => return Ok(value),
                Some(_) => {
                    return Err(GatewayError::InvalidResponse(
                        "stdio MCP response id does not match the request".into(),
                    ));
                }
                None => continue,
            }
        }
        Err(GatewayError::InvalidResponse(
            "stdio MCP sent no matching JSON-RPC response".into(),
        ))
    }

    async fn read_limited_line(
        &mut self,
        max_response_bytes: usize,
    ) -> Result<String, GatewayError> {
        let max_response_bytes = max_response_bytes.min(MAX_RESPONSE_BYTES);
        let mut line = Vec::new();
        loop {
            let available = self
                .stdout
                .fill_buf()
                .await
                .map_err(|error| GatewayError::Transport(error.to_string()))?;
            if available.is_empty() {
                return Err(GatewayError::Transport("stdio MCP closed stdout".into()));
            }
            if let Some(position) = available.iter().position(|byte| *byte == b'\n') {
                if line.len().saturating_add(position) > max_response_bytes {
                    return Err(GatewayError::ResponseTooLarge);
                }
                line.extend_from_slice(&available[..position]);
                self.stdout.consume(position + 1);
                break;
            }
            if line.len().saturating_add(available.len()) > max_response_bytes {
                return Err(GatewayError::ResponseTooLarge);
            }
            line.extend_from_slice(available);
            let length = available.len();
            self.stdout.consume(length);
        }
        String::from_utf8(line).map_err(|error| GatewayError::InvalidResponse(error.to_string()))
    }

    async fn initialize(&mut self, next_id: &AtomicU64) -> Result<(), GatewayError> {
        if self.initialized {
            return Ok(());
        }
        let id = next_id.fetch_add(1, Ordering::Relaxed);
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "software-factory-tool-gateway", "version": "0.1.0"}
            }
        }))
        .await?;
        let response = self
            .read_response(id, DEFAULT_TIMEOUT_SECS, MAX_RESPONSE_BYTES)
            .await?;
        if response
            .get("result")
            .and_then(|result| result.get("protocolVersion"))
            .is_none()
        {
            return Err(GatewayError::InvalidResponse(
                "stdio MCP initialize returned no protocol version".into(),
            ));
        }
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }))
        .await?;
        self.initialized = true;
        Ok(())
    }
}

struct StdioMcpClient {
    command: Vec<String>,
    env: BTreeMap<String, String>,
    process: tokio::sync::Mutex<Option<StdioProcess>>,
    next_id: AtomicU64,
}

impl StdioMcpClient {
    fn new(command: Vec<String>, env: BTreeMap<String, String>) -> Self {
        Self {
            command,
            env,
            process: tokio::sync::Mutex::new(None),
            next_id: AtomicU64::new(1),
        }
    }

    async fn call_tool(
        &self,
        tool: &str,
        arguments: Value,
        timeout_secs: u64,
        max_response_bytes: usize,
    ) -> Result<Value, GatewayError> {
        let mut guard = self.process.lock().await;
        if guard.is_none() {
            *guard = Some(StdioProcess::spawn(&self.command, &self.env)?);
        }
        let process = guard
            .as_mut()
            .ok_or_else(|| GatewayError::Transport("stdio MCP process is unavailable".into()))?;
        let operation = async {
            process.initialize(&self.next_id).await?;
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            process
                .send(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "tools/call",
                    "params": {"name": tool, "arguments": arguments}
                }))
                .await?;
            process
                .read_response(id, timeout_secs, max_response_bytes)
                .await
        };
        let outcome = timeout(
            Duration::from_secs(timeout_secs.saturating_add(1)),
            operation,
        )
        .await;
        let response = match outcome {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => {
                *guard = None;
                return Err(error);
            }
            Err(_) => {
                *guard = None;
                return Err(GatewayError::Transport(
                    "stdio MCP call exceeded its deadline".into(),
                ));
            }
        };
        let result = response
            .get("result")
            .cloned()
            .ok_or_else(|| GatewayError::InvalidResponse("stdio MCP returned no result".into()))?;
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            return Err(GatewayError::InvalidResponse(
                "stdio MCP tool reported an error".into(),
            ));
        }
        let bytes = serde_json::to_vec(&result)
            .map_err(|error| GatewayError::InvalidResponse(error.to_string()))?;
        if bytes.len() > max_response_bytes {
            return Err(GatewayError::ResponseTooLarge);
        }
        Ok(result)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GatewayOutcome {
    pub tool: String,
    pub server: String,
    pub content: Value,
    pub audit: GatewayAuditRecord,
}

#[derive(Debug)]
pub struct GatewayCallReport {
    pub outcome: Result<GatewayOutcome, GatewayError>,
    pub audit: GatewayAuditRecord,
}

pub struct ToolGateway {
    config: GatewayConfig,
    clients: BTreeMap<String, McpClient>,
    audit: AuditLog,
    limiter: RateLimiter,
}

impl ToolGateway {
    pub fn from_config(config: GatewayConfig) -> Result<Self, GatewayError> {
        config.validate()?;
        let mut clients = BTreeMap::new();
        for server in &config.servers {
            let (id, client) = match server {
                GatewayServerConfig::Http {
                    id,
                    endpoint,
                    bearer_env,
                    ..
                } => {
                    let auth = bearer_env
                        .as_ref()
                        .and_then(|variable| std::env::var(variable).ok())
                        .filter(|value| !value.trim().is_empty());
                    (
                        id.clone(),
                        McpClient::Http(HttpMcpClient::new(endpoint.clone(), auth)?),
                    )
                }
                GatewayServerConfig::Stdio { id, command, env } => (
                    id.clone(),
                    McpClient::Stdio(Box::new(StdioMcpClient::new(command.clone(), env.clone()))),
                ),
            };
            clients.insert(id, client);
        }
        Ok(Self {
            config,
            clients,
            audit: AuditLog::default(),
            limiter: RateLimiter::default(),
        })
    }

    pub fn from_env() -> Result<Option<Self>, GatewayError> {
        let path = match std::env::var("FACTORY_TOOL_GATEWAY_CONFIG") {
            Ok(path) if !path.trim().is_empty() => path,
            _ => return Ok(None),
        };
        let bytes = std::fs::read(&path)
            .map_err(|error| GatewayError::Config(format!("cannot read {path}: {error}")))?;
        if bytes.len() > 1024 * 1024 {
            return Err(GatewayError::Config("gateway config is too large".into()));
        }
        let config: GatewayConfig = serde_json::from_slice(&bytes)
            .map_err(|error| GatewayError::Config(error.to_string()))?;
        Self::from_config(config).map(Some)
    }

    pub fn example() -> Result<Self, GatewayError> {
        Self::from_config(GatewayConfig::example())
    }

    pub fn audit(&self) -> Vec<GatewayAuditRecord> {
        self.audit.snapshot()
    }

    pub async fn call(
        &self,
        role: GatewayRole,
        tool_id: &str,
        arguments: Value,
    ) -> Result<GatewayOutcome, GatewayError> {
        self.call_audited(role, tool_id, arguments).await.outcome
    }

    /// Call a tool and return the exact audit record even when policy denies or
    /// the transport fails. Core persists this record before returning the
    /// result to a Worker.
    pub async fn call_audited(
        &self,
        role: GatewayRole,
        tool_id: &str,
        arguments: Value,
    ) -> GatewayCallReport {
        let started = Instant::now();
        let args_sha256 = arguments_sha256(&arguments);
        let spec = match self.config.tools.iter().find(|tool| tool.id == tool_id) {
            Some(spec) => spec.clone(),
            None => {
                return self.deny(
                    role,
                    tool_id,
                    &args_sha256,
                    started,
                    GatewayError::UnknownTool(tool_id.into()),
                );
            }
        };
        // Dispatch the immutable Core identity, never the configured tool name.
        let Some(known) = known_tool(&spec.id) else {
            return self.deny(
                role,
                tool_id,
                &args_sha256,
                started,
                GatewayError::UnknownTool(tool_id.into()),
            );
        };
        if !role_allowed(&spec, role) {
            return self.deny(
                role,
                tool_id,
                &args_sha256,
                started,
                GatewayError::Denied {
                    role,
                    tool: tool_id.into(),
                },
            );
        }
        if let Err(error) = validate_arguments(&arguments) {
            return self.deny(role, tool_id, &args_sha256, started, error);
        }
        if let Err(error) = self.limiter.check(
            &format!("{role:?}:{tool_id}"),
            self.config.max_calls_per_minute,
        ) {
            return self.deny(role, tool_id, &args_sha256, started, error);
        }
        let client = match self.clients.get(&spec.server) {
            Some(client) => client,
            None => {
                return self.deny(
                    role,
                    tool_id,
                    &args_sha256,
                    started,
                    GatewayError::NotConfigured,
                );
            }
        };
        match client
            .call_tool(
                known.tool,
                arguments,
                spec.timeout_secs.min(MAX_TIMEOUT_SECS),
                spec.max_response_bytes.min(MAX_RESPONSE_BYTES),
            )
            .await
        {
            Ok(content) => {
                let response_bytes = serde_json::to_vec(&content)
                    .map(|bytes| bytes.len())
                    .unwrap_or(0);
                let audit = self.record(
                    role,
                    tool_id,
                    &args_sha256,
                    started,
                    "ALLOW",
                    response_bytes,
                    None,
                );
                GatewayCallReport {
                    outcome: Ok(GatewayOutcome {
                        tool: tool_id.into(),
                        server: spec.server,
                        content,
                        audit: audit.clone(),
                    }),
                    audit,
                }
            }
            Err(error) => {
                let audit = self.record(
                    role,
                    tool_id,
                    &args_sha256,
                    started,
                    "ALLOW",
                    0,
                    Some(&error.to_string()),
                );
                GatewayCallReport {
                    outcome: Err(error),
                    audit,
                }
            }
        }
    }

    fn deny(
        &self,
        role: GatewayRole,
        tool: &str,
        args_sha256: &str,
        started: Instant,
        error: GatewayError,
    ) -> GatewayCallReport {
        let audit = self.record(
            role,
            tool,
            args_sha256,
            started,
            "DENY",
            0,
            Some(&error.to_string()),
        );
        GatewayCallReport {
            outcome: Err(error),
            audit,
        }
    }

    /// Role-filtered catalog for a provider prompt. It never includes a tool
    /// that `call` would deny for that role.
    pub fn tools_for_role(&self, role: GatewayRole) -> Vec<GatewayToolSummary> {
        self.config
            .tools
            .iter()
            .filter(|spec| role_allowed(spec, role))
            .filter_map(|spec| {
                known_tool(&spec.id).map(|known| GatewayToolSummary {
                    id: known.id.into(),
                    server: known.server.into(),
                    read_only: known.read_only,
                    hint: known.hint.into(),
                })
            })
            .collect()
    }

    pub fn status(&self) -> GatewayStatus {
        GatewayStatus {
            enabled: true,
            servers: self
                .config
                .servers
                .iter()
                .map(|server| match server {
                    GatewayServerConfig::Http { id, .. } => GatewayServerSummary {
                        id: id.clone(),
                        transport: "http".into(),
                    },
                    GatewayServerConfig::Stdio { id, .. } => GatewayServerSummary {
                        id: id.clone(),
                        transport: "stdio".into(),
                    },
                })
                .collect(),
            tools: self
                .config
                .tools
                .iter()
                .filter_map(|spec| {
                    known_tool(&spec.id).map(|known| GatewayToolSummary {
                        id: known.id.into(),
                        server: known.server.into(),
                        read_only: known.read_only,
                        hint: known.hint.into(),
                    })
                })
                .collect(),
            max_calls_per_minute: self.config.max_calls_per_minute,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        &self,
        role: GatewayRole,
        tool: &str,
        args_sha256: &str,
        started: Instant,
        decision: &str,
        response_bytes: usize,
        error: Option<&str>,
    ) -> GatewayAuditRecord {
        self.audit.record(GatewayAuditRecord {
            role,
            tool: tool.into(),
            args_sha256: args_sha256.into(),
            decision: decision.into(),
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            response_bytes,
            error: error.map(str::to_owned),
        })
    }
}

fn arguments_sha256(arguments: &Value) -> String {
    let bytes = serde_json::to_vec(arguments).unwrap_or_default();
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn validate_arguments(arguments: &Value) -> Result<(), GatewayError> {
    let bytes = serde_json::to_vec(arguments)
        .map_err(|error| GatewayError::InvalidArguments(error.to_string()))?;
    if bytes.len() > MAX_ARGUMENT_BYTES {
        return Err(GatewayError::InvalidArguments(
            "arguments exceed the bound".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_gateway() -> ToolGateway {
        let mut config = GatewayConfig::example();
        config.servers = vec![
            GatewayServerConfig::Http {
                id: "context7".into(),
                endpoint: "http://127.0.0.1:9/mcp".into(),
                bearer_env: None,
                allow_loopback_http: true,
            },
            GatewayServerConfig::Stdio {
                id: "playwright".into(),
                command: vec!["true".into()],
                env: BTreeMap::new(),
            },
        ];
        config.tools = vec![
            tool(
                "context7.query-docs",
                "context7",
                "query-docs",
                &[GatewayRole::Implementer, GatewayRole::Reviewer],
                true,
            ),
            tool(
                "playwright.browser_close",
                "playwright",
                "browser_close",
                &[GatewayRole::Implementer],
                false,
            ),
        ];
        ToolGateway::from_config(config).expect("gateway")
    }

    #[test]
    fn denies_unknown_tools_and_disallowed_roles() {
        let gateway = test_gateway();
        assert!(!gateway.config.tools.iter().any(|tool| tool.id == "missing"));
        assert!(matches!(
            gateway
                .config
                .tools
                .iter()
                .find(|tool| tool.id == "context7.query-docs")
                .map(|tool| tool.roles.contains(&GatewayRole::Verifier)),
            Some(false)
        ));
    }

    #[test]
    fn reviewer_may_not_use_a_mutating_browser_tool() {
        let gateway = test_gateway();
        let spec = gateway
            .config
            .tools
            .iter()
            .find(|tool| tool.id == "playwright.browser_close")
            .expect("tool");
        assert!(!spec.roles.contains(&GatewayRole::Reviewer));
        assert!(!core_tool_is_read_only(spec));
    }

    #[test]
    fn arguments_are_hashed_and_bounded() {
        let arguments = json!({"query": "react hooks"});
        let digest = arguments_sha256(&arguments);
        assert_eq!(digest.len(), 64);
        let oversized = json!({"query": "x".repeat(MAX_ARGUMENT_BYTES + 1)});
        assert!(matches!(
            validate_arguments(&oversized),
            Err(GatewayError::InvalidArguments(_))
        ));
    }

    #[tokio::test]
    async fn http_transport_initializes_and_calls_a_tool() {
        use axum::{Json, Router, routing::post};

        async fn handler(Json(body): Json<Value>) -> Json<Value> {
            let id = body.get("id").cloned().unwrap_or(Value::Null);
            if body.get("method").and_then(Value::as_str) == Some("initialize") {
                return Json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {"protocolVersion": "2025-03-26"}
                }));
            }
            if body.get("method").and_then(Value::as_str) == Some("tools/call") {
                return Json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {"content": [{"type": "text", "text": "ok"}]}
                }));
            }
            Json(json!({"jsonrpc": "2.0", "id": id, "result": {}}))
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/mcp", post(handler)))
                .await
                .expect("fake MCP server");
        });

        let mut config = GatewayConfig::example();
        config.servers = vec![GatewayServerConfig::Http {
            id: "context7".into(),
            endpoint: format!("http://{address}/mcp"),
            bearer_env: None,
            allow_loopback_http: true,
        }];
        config.tools = vec![tool(
            "context7.query-docs",
            "context7",
            "query-docs",
            &[GatewayRole::Implementer],
            true,
        )];
        let gateway = ToolGateway::from_config(config).expect("gateway");
        let outcome = gateway
            .call(
                GatewayRole::Implementer,
                "context7.query-docs",
                json!({"query": "react hooks"}),
            )
            .await
            .expect("MCP call");
        assert_eq!(outcome.content["content"][0]["text"], "ok");
        assert_eq!(gateway.audit().last().expect("audit").decision, "ALLOW");
    }

    #[tokio::test]
    async fn http_applies_the_tool_response_bound_while_reading() {
        use axum::{Json, Router, routing::post};

        async fn handler(Json(body): Json<Value>) -> Json<Value> {
            let id = body.get("id").cloned().unwrap_or(Value::Null);
            if body.get("method").and_then(Value::as_str) == Some("initialize") {
                return Json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {"protocolVersion": "2025-03-26"}
                }));
            }
            Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"content": [{"type": "text", "text": "x".repeat(4096)}]}
            }))
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/mcp", post(handler)))
                .await
                .expect("fake MCP server");
        });
        let mut config = GatewayConfig::example();
        config.servers = vec![GatewayServerConfig::Http {
            id: "context7".into(),
            endpoint: format!("http://{address}/mcp"),
            bearer_env: None,
            allow_loopback_http: true,
        }];
        config.tools = vec![tool(
            "context7.query-docs",
            "context7",
            "query-docs",
            &[GatewayRole::Implementer],
            true,
        )];
        config.tools[0].max_response_bytes = 512;
        let gateway = ToolGateway::from_config(config).expect("gateway");
        assert!(matches!(
            gateway
                .call(
                    GatewayRole::Implementer,
                    "context7.query-docs",
                    json!({"query": "react"})
                )
                .await,
            Err(GatewayError::ResponseTooLarge)
        ));
    }

    #[tokio::test]
    async fn http_tool_timeout_covers_initialization() {
        use axum::{Json, Router, routing::post};

        async fn handler(Json(body): Json<Value>) -> Json<Value> {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let id = body.get("id").cloned().unwrap_or(Value::Null);
            Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"protocolVersion": "2025-03-26"}
            }))
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/mcp", post(handler)))
                .await
                .expect("fake MCP server");
        });
        let mut config = GatewayConfig::example();
        config.servers = vec![GatewayServerConfig::Http {
            id: "context7".into(),
            endpoint: format!("http://{address}/mcp"),
            bearer_env: None,
            allow_loopback_http: true,
        }];
        config.tools = vec![tool(
            "context7.query-docs",
            "context7",
            "query-docs",
            &[GatewayRole::Implementer],
            true,
        )];
        config.tools[0].timeout_secs = 1;
        let gateway = ToolGateway::from_config(config).expect("gateway");
        assert!(matches!(
            gateway
                .call(
                    GatewayRole::Implementer,
                    "context7.query-docs",
                    json!({"query": "react"})
                )
                .await,
            Err(GatewayError::Transport(message)) if message.contains("timed out")
        ));
    }

    #[tokio::test]
    async fn stdio_transport_initializes_and_calls_a_tool() {
        let script = r#"
import json, sys
for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if method == "initialize":
        print(json.dumps({"jsonrpc":"2.0","id":request.get("id"),"result":{"protocolVersion":"2025-03-26"}}), flush=True)
    elif method == "tools/call":
        print(json.dumps({"jsonrpc":"2.0","method":"notifications/progress","params":{}}), flush=True)
        print(json.dumps({"jsonrpc":"2.0","id":request.get("id"),"result":{"content":[{"type":"text","text":"stdio-ok"}]}}), flush=True)
"#;
        let mut config = GatewayConfig::example();
        config.servers = vec![GatewayServerConfig::Stdio {
            id: "playwright".into(),
            command: vec!["python3".into(), "-u".into(), "-c".into(), script.into()],
            env: BTreeMap::new(),
        }];
        config.tools = vec![tool(
            "playwright.browser_snapshot",
            "playwright",
            "browser_snapshot",
            &[GatewayRole::Implementer],
            true,
        )];
        let gateway = ToolGateway::from_config(config).expect("gateway");
        let outcome = gateway
            .call(
                GatewayRole::Implementer,
                "playwright.browser_snapshot",
                json!({}),
            )
            .await
            .expect("stdio MCP call");
        assert_eq!(outcome.content["content"][0]["text"], "stdio-ok");
    }

    #[test]
    fn example_catalog_parses_and_validates() {
        let config: GatewayConfig =
            serde_json::from_str(include_str!("../fixtures/tool-gateway.example.json"))
                .expect("example config");
        config.validate().expect("valid example catalog");
        assert!(
            config
                .tools
                .iter()
                .any(|tool| tool.id == "playwright.browser_snapshot")
        );
        assert!(
            config
                .tools
                .iter()
                .any(|tool| tool.id == "github.issue_read")
        );
    }

    #[test]
    fn rejects_unknown_tool_identities_and_server_remapping() {
        let mut config = GatewayConfig::example();
        config.tools = vec![GatewayToolSpec {
            id: "context7.arbitrary".into(),
            server: "context7".into(),
            tool: "arbitrary".into(),
            roles: vec![GatewayRole::Reviewer],
            read_only: true,
            timeout_secs: 30,
            max_response_bytes: 1024,
        }];
        assert!(matches!(config.validate(), Err(GatewayError::Config(_))));

        config.tools[0].id = "context7.query-docs".into();
        config.tools[0].server = "github-read".into();
        config.tools[0].tool = "query-docs".into();
        assert!(matches!(config.validate(), Err(GatewayError::Config(_))));

        // A known id must keep its exact MCP tool name even for the right server.
        let mut config = GatewayConfig::example();
        config.tools = vec![tool(
            "playwright.browser_snapshot",
            "playwright",
            "browser_click",
            &[GatewayRole::Reviewer],
            true,
        )];
        assert!(matches!(
            config.validate(),
            Err(GatewayError::Config(message)) if message.contains("MCP tool")
        ));

        let mut config = GatewayConfig::example();
        config.servers = vec![GatewayServerConfig::Http {
            id: "context7".into(),
            endpoint: "https://evil.example/mcp".into(),
            bearer_env: None,
            allow_loopback_http: false,
        }];
        assert!(matches!(config.validate(), Err(GatewayError::Config(_))));
    }

    #[test]
    fn rejects_bounds_above_the_transport_limits() {
        let mut config = GatewayConfig::example();
        config.tools[0].max_response_bytes = MAX_RESPONSE_BYTES + 1;
        assert!(matches!(
            config.validate(),
            Err(GatewayError::Config(message)) if message.contains("response bound")
        ));
        config.tools[0].max_response_bytes = MAX_RESPONSE_BYTES;
        config.tools[0].timeout_secs = MAX_TIMEOUT_SECS + 1;
        assert!(matches!(
            config.validate(),
            Err(GatewayError::Config(message)) if message.contains("timeout")
        ));
        config.tools[0].timeout_secs = MAX_TIMEOUT_SECS;
        config.validate().expect("limits at the ceiling are valid");
    }

    #[tokio::test]
    async fn role_catalog_matches_call_policy_and_denials_carry_audit() {
        let gateway = ToolGateway::example().expect("gateway");
        let implementer = gateway.tools_for_role(GatewayRole::Implementer);
        assert!(
            implementer
                .iter()
                .any(|tool| tool.id == "playwright.browser_close")
        );
        let reviewer = gateway.tools_for_role(GatewayRole::Reviewer);
        assert!(
            !reviewer
                .iter()
                .any(|tool| tool.id == "playwright.browser_close")
        );
        assert!(reviewer.iter().all(|tool| tool.read_only));
        assert!(gateway.tools_for_role(GatewayRole::Verifier).is_empty());
        assert!(gateway.status().enabled);
        assert_eq!(gateway.status().tools.len(), 8);

        let report = gateway
            .call_audited(
                GatewayRole::Verifier,
                "context7.query-docs",
                json!({"query": "x"}),
            )
            .await;
        assert!(matches!(report.outcome, Err(GatewayError::Denied { .. })));
        assert_eq!(report.audit.decision, "DENY");
        assert_eq!(report.audit.role, GatewayRole::Verifier);
    }

    #[test]
    fn rejects_verifier_and_mutating_reviewer_grants() {
        let mut config = GatewayConfig::example();
        config.servers = vec![GatewayServerConfig::Stdio {
            id: "playwright".into(),
            command: vec!["true".into()],
            env: BTreeMap::new(),
        }];
        config.tools = vec![tool(
            "playwright.browser_close",
            "playwright",
            "browser_close",
            &[GatewayRole::Verifier],
            true,
        )];
        assert!(matches!(
            config.validate(),
            Err(GatewayError::Config(message)) if message.contains("Verifier")
        ));
        config.tools[0].roles = vec![GatewayRole::Reviewer];
        assert!(matches!(
            config.validate(),
            Err(GatewayError::Config(message)) if message.contains("mutating")
        ));
    }

    #[tokio::test]
    async fn stdio_child_does_not_inherit_the_gateway_environment() {
        let script = r#"
import json, os, sys
for line in sys.stdin:
    request = json.loads(line)
    if request.get("method") == "initialize":
        print(json.dumps({"jsonrpc":"2.0","id":request.get("id"),"result":{"protocolVersion":"2025-03-26"}}), flush=True)
    elif request.get("method") == "tools/call":
        print(json.dumps({"jsonrpc":"2.0","id":request.get("id"),"result":{"content":[{"type":"text","text":str(os.environ.get("PATH"))}]}}), flush=True)
"#;
        let mut config = GatewayConfig::example();
        config.servers = vec![GatewayServerConfig::Stdio {
            id: "playwright".into(),
            command: vec!["python3".into(), "-u".into(), "-c".into(), script.into()],
            env: BTreeMap::new(),
        }];
        config.tools = vec![tool(
            "playwright.browser_snapshot",
            "playwright",
            "browser_snapshot",
            &[GatewayRole::Implementer],
            true,
        )];
        let gateway = ToolGateway::from_config(config).expect("gateway");
        let outcome = gateway
            .call(
                GatewayRole::Implementer,
                "playwright.browser_snapshot",
                json!({}),
            )
            .await
            .expect("stdio MCP call");
        assert_eq!(outcome.content["content"][0]["text"], "None");
    }

    #[tokio::test]
    async fn stdio_rejects_an_oversized_line() {
        let script = r#"
import json, sys
for line in sys.stdin:
    request = json.loads(line)
    if request.get("method") == "initialize":
        print(json.dumps({"jsonrpc":"2.0","id":request.get("id"),"result":{"protocolVersion":"2025-03-26"}}), flush=True)
    elif request.get("method") == "tools/call":
        print(json.dumps({"jsonrpc":"2.0","id":request.get("id"),"result":{"content":[{"type":"text","text":"x"*300000}]}}), flush=True)
"#;
        let mut config = GatewayConfig::example();
        config.servers = vec![GatewayServerConfig::Stdio {
            id: "playwright".into(),
            command: vec!["python3".into(), "-u".into(), "-c".into(), script.into()],
            env: BTreeMap::new(),
        }];
        config.tools = vec![tool(
            "playwright.browser_snapshot",
            "playwright",
            "browser_snapshot",
            &[GatewayRole::Implementer],
            true,
        )];
        config.tools[0].max_response_bytes = 1024;
        let gateway = ToolGateway::from_config(config).expect("gateway");
        assert!(matches!(
            gateway
                .call(
                    GatewayRole::Implementer,
                    "playwright.browser_snapshot",
                    json!({})
                )
                .await,
            Err(GatewayError::ResponseTooLarge)
        ));
    }

    #[tokio::test]
    async fn http_rejects_a_mismatched_json_rpc_id() {
        use axum::{Json, Router, routing::post};

        async fn handler(Json(body): Json<Value>) -> Json<Value> {
            let method = body.get("method").and_then(Value::as_str);
            if method == Some("initialize") {
                return Json(json!({
                    "jsonrpc": "2.0",
                    "id": body.get("id").cloned().unwrap_or(Value::Null),
                    "result": {"protocolVersion": "2025-03-26"}
                }));
            }
            Json(json!({
                "jsonrpc": "2.0",
                "id": 999,
                "result": {"content": [{"type": "text", "text": "wrong id"}]}
            }))
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/mcp", post(handler)))
                .await
                .expect("fake MCP server");
        });
        let mut config = GatewayConfig::example();
        config.servers = vec![GatewayServerConfig::Http {
            id: "context7".into(),
            endpoint: format!("http://{address}/mcp"),
            bearer_env: None,
            allow_loopback_http: true,
        }];
        config.tools = vec![tool(
            "context7.query-docs",
            "context7",
            "query-docs",
            &[GatewayRole::Implementer],
            true,
        )];
        let gateway = ToolGateway::from_config(config).expect("gateway");
        assert!(matches!(
            gateway
                .call(
                    GatewayRole::Implementer,
                    "context7.query-docs",
                    json!({"query": "react"})
                )
                .await,
            Err(GatewayError::InvalidResponse(_))
        ));
    }

    #[test]
    fn rate_limit_is_per_role_and_tool() {
        let limiter = RateLimiter::default();
        limiter
            .check("IMPLEMENTER:context7.query-docs", 1)
            .expect("first");
        assert!(matches!(
            limiter.check("IMPLEMENTER:context7.query-docs", 1),
            Err(GatewayError::RateLimited(_))
        ));
        limiter
            .check("REVIEWER:context7.query-docs", 1)
            .expect("other role");
    }
}
