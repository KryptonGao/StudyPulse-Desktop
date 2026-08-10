use async_trait::async_trait;
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use thiserror::Error;
use uuid::Uuid;

pub const DEFAULT_CLOUD_API_BASE_URL: &str = "https://spapi.chenkai.space";
pub const DEFAULT_CLOUD_AUTH_BASE_URL: &str = "https://auth.chenkai.space";
pub const DEFAULT_OPENAI_COMPATIBLE_BASE_URL: &str = "https://api.openai.com/v1";
const MAX_CLOUD_MESSAGE_CHARACTERS: usize = 32_768;

// This crate normalizes several provider protocols into one host-owned model
// contract.  The Agent sends structured messages and tool schemas here; the
// provider may stream text or return tool-call JSON, but it never executes a
// tool and never decides how credentials or errors are exposed to the UI.
//
// Cloud AI and OpenAI-compatible BYOK intentionally share the streaming and
// error-normalization helpers while retaining their distinct authentication and
// request shapes.

// The public records in this file are intentionally provider-neutral.  They
// are the seam shared by the Agent loop, Tauri commands, and future clients;
// provider-specific envelopes are kept below the client implementations so a
// new endpoint does not require changing the host-facing conversation model.

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    /// The model does not execute tools directly, but exposing the host-side
    /// permission class makes the tool contract much easier for weaker models
    /// to follow.  Keep this optional so older clients can still deserialize
    /// requests produced before the Agent protocol gained permission metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission: Option<String>,
}

// Tool calls are provider-neutral before they reach the Agent runtime.  The
// arguments remain JSON because each registered tool owns its own strict schema
// and the host performs the authoritative validation after this layer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum ChatMessage {
    User {
        content: String,
    },
    Assistant {
        content: String,
    },
    Tool {
        call_id: String,
        name: String,
        content: String,
    },
}

// Tool messages are emitted by the host after execution, never accepted as
// arbitrary caller input.  That distinction lets a provider continue a loop
// from an authoritative result instead of manufacturing its own tool output.

// `ChatMessage` is a tagged wire enum.  Keeping the role tag and camelCase
// request fields stable lets Rust, Tauri, and Swift share the same transcript
// without per-provider role conversions leaking into the FFI layer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelRequest {
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ModelToolDefinition>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub stages: Vec<String>,
}

// A response can contain both streamed text snapshots and tool calls.  The
// runtime emits callbacks during transport, then uses this final object to
// recover any suffix not already sent and to continue the tool loop.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelResponse {
    pub text_deltas: Vec<String>,
    pub tool_calls: Vec<ModelToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<ModelUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub estimated: bool,
}

// `text_deltas` is a final normalized snapshot, not necessarily a one-to-one
// copy of transport chunks.  The Agent compares it with callback text to avoid
// displaying the same streamed answer twice.

pub type ModelTextDeltaHandler = Arc<dyn Fn(String) + Send + Sync>;

// Cloud tokens and profiles stay in Rust-owned state and are converted to
// redacted DTOs by the outer facade.  These structs therefore carry values for
// provider calls but are not serialized as part of a model request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudAuthTokens {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudProfile {
    pub email: String,
    pub role: String,
    pub membership_type: String,
    pub membership_expires_at: Option<String>,
    pub plan_name: String,
    pub available_models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudSession {
    pub tokens: CloudAuthTokens,
    pub profile: CloudProfile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByokConfig {
    pub base_url: String,
    pub model: String,
}

// Errors form the stable classification boundary used by Agent and UI code.
// Keep authentication, quota, access, payload, response, and transport cases
// distinct so callers can choose recovery actions without parsing strings.
#[derive(Debug, Error)]
pub enum ModelError {
    #[error("Cloud AI is not configured")]
    NotConfigured,
    #[error("Cloud AI URL is invalid: {0}")]
    InvalidUrl(String),
    #[error("Cloud AI authentication callback is invalid")]
    InvalidAuthCallback,
    #[error("Cloud AI login was rejected: {0}")]
    AuthRejected(String),
    #[error("Cloud AI session expired; sign in again")]
    SessionExpired,
    #[error("Cloud AI quota is exhausted: {0}")]
    QuotaExceeded(String),
    #[error("Cloud AI access was denied: {0}")]
    AccessDenied(String),
    #[error("Cloud AI request is too large")]
    RequestTooLarge,
    #[error("Cloud AI returned an invalid response")]
    InvalidResponse,
    #[error("model request failed: {0}")]
    Request(String),
}

#[async_trait]
pub trait ModelClient: Send + Sync {
    // Implementations must invoke the callback with incremental user-visible
    // text when streaming is available, then return the normalized response.
    // Tool-call JSON is deliberately excluded from the text callback.
    async fn complete(
        &self,
        request: ModelRequest,
        on_text_delta: ModelTextDeltaHandler,
    ) -> Result<ModelResponse, ModelError>;
}

// The mock is deliberately protocol-shaped rather than a shortcut that calls
// Workspace itself.  It proposes the same read/read/write sequence as a model,
// leaving permission prompts and side-effect assertions to the real runtime.

#[derive(Debug, Default)]
pub struct MockModelClient;

#[async_trait]
impl ModelClient for MockModelClient {
    // The mock progresses through the same list/read/write sequence as a real
    // Agent run.  It is deterministic and network-free, making consent and
    // cursor tests exercise the runtime rather than a provider.
    async fn complete(
        &self,
        request: ModelRequest,
        _on_text_delta: ModelTextDeltaHandler,
    ) -> Result<ModelResponse, ModelError> {
        let completed_tools = request
            .messages
            .iter()
            .filter(|message| matches!(message, ChatMessage::Tool { .. }))
            .count();
        let goal = request
            .messages
            .iter()
            .find_map(|message| match message {
                ChatMessage::User { content } => Some(content.trim()),
                _ => None,
            })
            .filter(|value| !value.is_empty())
            .unwrap_or("Study task");
        let response = match completed_tools {
            0 => tool_call("list_workspace_files", json!({})),
            1 => tool_call("get_tasks", json!({})),
            2 => tool_call(
                "create_task",
                json!({
                    "title": goal,
                    "task_type": "homework",
                    "importance": 3
                }),
            ),
            _ => ModelResponse {
                text_deltas: vec![
                    "已完成这个学习目标的准备工作。".into(),
                    "任务已安全写入当前 Workspace，".into(),
                    "你可以在 Tasks 页面查看。".into(),
                ],
                tool_calls: Vec::new(),
                usage: None,
            },
        };
        Ok(response)
    }
}

#[derive(Debug, Clone)]
pub struct CloudModelClient {
    http: Client,
    api_base_url: Url,
    access_token: String,
    model: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OpenAICompatibleModelClient {
    http: Client,
    api_base_url: Url,
    api_key: String,
    model: String,
}

// Cloud and BYOK clients share transport helpers but keep credentials in
// separate structs.  This prevents a status/configuration call from confusing
// a Cloud session token with a user-provided API key.

impl CloudModelClient {
    // Cloud construction does not perform a network request.  Keeping setup
    // local makes login/profile/refresh explicit operations and allows the
    // caller to surface configuration errors before starting Agent work.
    // Construction validates only local configuration and normalizes the base
    // URL.  Token validity is established by the auth flow or a later request,
    // while the token itself never enters a serialized DTO.
    pub fn new(
        api_base_url: &str,
        access_token: String,
        model: Option<String>,
    ) -> Result<Self, ModelError> {
        if access_token.trim().is_empty() {
            return Err(ModelError::NotConfigured);
        }
        Ok(Self {
            http: cloud_http_client()?,
            api_base_url: normalize_base_url(api_base_url)?,
            access_token,
            model: model.filter(|value| !value.trim().is_empty()),
        })
    }

    pub fn login_url(callback_url: &str) -> Result<String, ModelError> {
        // The deep-link callback is validated before it is embedded in the
        // login URL, preventing an arbitrary redirect from entering the auth
        // handshake.
        validate_callback_base(callback_url)?;
        let mut url = normalize_base_url(DEFAULT_CLOUD_AUTH_BASE_URL)?
            .join("login")
            .map_err(|error| ModelError::InvalidUrl(error.to_string()))?;
        url.query_pairs_mut().append_pair("return_to", callback_url);
        Ok(url.into())
    }

    pub fn parse_auth_callback(callback_url: &str) -> Result<CloudAuthTokens, ModelError> {
        // Accept only the registered studypulse://auth/callback shape and the
        // provider-specific token prefixes.  Auth rejection is kept separate
        // from malformed callbacks so UI can offer the right recovery action.
        let url = Url::parse(callback_url).map_err(|_| ModelError::InvalidAuthCallback)?;
        if url.scheme() != "studypulse"
            || url.host_str() != Some("auth")
            || url.path() != "/callback"
        {
            return Err(ModelError::InvalidAuthCallback);
        }
        let values = url
            .query_pairs()
            .collect::<std::collections::HashMap<_, _>>();
        if let Some(error) = values.get("error") {
            let message = values
                .get("error_description")
                .map_or_else(|| error.as_ref(), AsRef::as_ref);
            return Err(ModelError::AuthRejected(message.to_owned()));
        }
        let access_token = values
            .get("access_token")
            .filter(|value| value.starts_with("sp_sess_"))
            .ok_or(ModelError::InvalidAuthCallback)?
            .to_string();
        let refresh_token = values
            .get("refresh_token")
            .filter(|value| value.starts_with("sp_refresh_"))
            .ok_or(ModelError::InvalidAuthCallback)?
            .to_string();
        Ok(CloudAuthTokens {
            access_token,
            refresh_token,
        })
    }

    pub async fn profile(&self) -> Result<CloudProfile, ModelError> {
        // Profile responses are status data, not authorization data.  The
        // access token is used only by the request client and never copied into
        // the returned CloudProfile or any facade DTO.
        // Profile data is a capability/status view, not a credential store.  A
        // successful HTTP response still requires a valid provider envelope and
        // falls back only for optional membership/plan fields.
        let url = self
            .api_base_url
            .join("user/profile")
            .map_err(|error| ModelError::InvalidUrl(error.to_string()))?;
        let response = self
            .http
            .get(url)
            .bearer_auth(&self.access_token)
            .send()
            .await
            .map_err(request_error)?;
        let status = response.status();
        let bytes = response.bytes().await.map_err(request_error)?;
        if !status.is_success() {
            return Err(map_cloud_error(status, &bytes));
        }
        let envelope: ProfileEnvelope =
            serde_json::from_slice(&bytes).map_err(|_| ModelError::InvalidResponse)?;
        let data = envelope
            .data
            .filter(|_| envelope.success)
            .ok_or(ModelError::InvalidResponse)?;
        Ok(CloudProfile {
            email: data.email.unwrap_or_default(),
            role: data.role.unwrap_or_else(|| "user".into()),
            membership_type: data
                .membership
                .as_ref()
                .and_then(|value| {
                    value
                        .effective_type
                        .clone()
                        .or(value.membership_type.clone())
                })
                .unwrap_or_else(|| "free".into()),
            membership_expires_at: data.membership.and_then(|value| value.expires_at),
            plan_name: data
                .plan
                .as_ref()
                .and_then(|value| value.name.clone())
                .unwrap_or_else(|| "Free".into()),
            available_models: data
                .plan
                .and_then(|value| value.available_models)
                .unwrap_or_default(),
        })
    }

    pub async fn refresh_session(
        api_base_url: &str,
        refresh_token: &str,
    ) -> Result<CloudAuthTokens, ModelError> {
        // Refresh accepts known server envelope variants for compatibility, but
        // the prefix checks below remain strict so an arbitrary response value
        // cannot be promoted into a usable session token.
        // The refresh endpoint has appeared with several compatible envelope
        // shapes; accept the known token field variants but require the same
        // access/refresh prefixes before returning them to the host.
        if !refresh_token.starts_with("sp_refresh_") {
            return Err(ModelError::SessionExpired);
        }
        let url = normalize_base_url(api_base_url)?
            .join("auth/refresh")
            .map_err(|error| ModelError::InvalidUrl(error.to_string()))?;
        let response = cloud_http_client()?
            .post(url)
            .json(&json!({ "refresh_token": refresh_token }))
            .send()
            .await
            .map_err(request_error)?;
        let status = response.status();
        let bytes = response.bytes().await.map_err(request_error)?;
        if !status.is_success() {
            return Err(map_cloud_error(status, &bytes));
        }
        let envelope: RefreshEnvelope =
            serde_json::from_slice(&bytes).map_err(|_| ModelError::InvalidResponse)?;
        let data = envelope.data.unwrap_or_default();
        let access_token = envelope
            .access_token
            .or(data.access_token)
            .or(data.session_token)
            .or(data.token)
            .filter(|value| value.starts_with("sp_sess_"))
            .ok_or(ModelError::InvalidResponse)?;
        let refresh_token = envelope
            .refresh_token
            .or(data.refresh_token)
            .filter(|value| value.starts_with("sp_refresh_"))
            .ok_or(ModelError::InvalidResponse)?;
        Ok(CloudAuthTokens {
            access_token,
            refresh_token,
        })
    }

    pub async fn logout(&self) -> Result<(), ModelError> {
        // Logout treats an already-invalid session as complete, making the UI
        // idempotent after remote revocation instead of trapping the account in
        // a signed-in state because the provider returned 401.
        // Logout is idempotent for an already-expired session.  Treating 401 as
        // success avoids trapping the user in a signed-in UI after revocation.
        let url = self
            .api_base_url
            .join("v1/auth/logout")
            .map_err(|error| ModelError::InvalidUrl(error.to_string()))?;
        let response = self
            .http
            .post(url)
            .bearer_auth(&self.access_token)
            .send()
            .await
            .map_err(request_error)?;
        if response.status().is_success() || response.status() == StatusCode::UNAUTHORIZED {
            return Ok(());
        }
        let status = response.status();
        let bytes = response.bytes().await.map_err(request_error)?;
        Err(map_cloud_error(status, &bytes))
    }

    async fn send_chat_stream(
        &self,
        message: String,
        on_text_delta: &ModelTextDeltaHandler,
    ) -> Result<String, ModelError> {
        // Cloud's stream is parsed as a text transport and normalized into the
        // same final/tool protocol used by BYOK.  Provider-specific event names
        // stop at this method boundary.
        // Cloud uses a compact `{message, stream}` envelope.  The response may
        // be JSON fallback or SSE; both paths end at StreamingAgentReply so the
        // Agent sees one final text representation.
        if message.chars().count() > MAX_CLOUD_MESSAGE_CHARACTERS {
            return Err(ModelError::RequestTooLarge);
        }
        let url = self
            .api_base_url
            .join("v1/chat")
            .map_err(|error| ModelError::InvalidUrl(error.to_string()))?;
        let mut body = json!({ "message": message, "stream": true });
        if let Some(model) = &self.model {
            body["model"] = Value::String(model.clone());
        }
        let mut response = self
            .http
            .post(url)
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await
            .map_err(request_error)?;
        let status = response.status();
        if !status.is_success() {
            let bytes = response.bytes().await.map_err(request_error)?;
            return Err(map_cloud_error(status, &bytes));
        }

        let is_event_stream = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream"));
        if !is_event_stream {
            let bytes = response.bytes().await.map_err(request_error)?;
            let envelope: ChatEnvelope =
                serde_json::from_slice(&bytes).map_err(|_| ModelError::InvalidResponse)?;
            return envelope
                .data
                .and_then(|value| value.reply)
                .filter(|value| !value.trim().is_empty())
                .ok_or(ModelError::InvalidResponse);
        }

        let mut event_buffer = Vec::new();
        let mut streamed_reply = StreamingAgentReply::default();
        while let Some(chunk) = response.chunk().await.map_err(request_error)? {
            event_buffer.extend_from_slice(&chunk);
            while let Some((boundary, separator_length)) = sse_event_boundary(&event_buffer) {
                let event = event_buffer[..boundary].to_vec();
                event_buffer.drain(..boundary + separator_length);
                if consume_sse_event(&event, &mut streamed_reply, on_text_delta)? {
                    return streamed_reply.finish();
                }
            }
        }
        if !event_buffer.is_empty() {
            consume_sse_event(&event_buffer, &mut streamed_reply, on_text_delta)?;
        }
        streamed_reply.finish()
    }
}

impl OpenAICompatibleModelClient {
    // BYOK construction normalizes only endpoint metadata.  The key stays in
    // the client instance and is used for authorization/redaction, never for
    // status projection or prompt content.
    // BYOK requires a non-empty key and model, then stores a normalized base URL
    // for later `/chat/completions` resolution.  The key remains private to the
    // client and is redacted if a provider echoes it in an error body.
    pub fn new(api_base_url: &str, api_key: String, model: String) -> Result<Self, ModelError> {
        if api_key.trim().is_empty() {
            return Err(ModelError::NotConfigured);
        }
        let model = model.trim();
        if model.is_empty() {
            return Err(ModelError::Request("BYOK model is required".into()));
        }
        Ok(Self {
            http: cloud_http_client()?,
            api_base_url: normalize_base_url(api_base_url)?,
            api_key,
            model: model.into(),
        })
    }

    pub fn config(&self) -> ByokConfig {
        // Expose only reconnectable non-secret settings.  The API key is never
        // returned through this status/configuration view.
        ByokConfig {
            base_url: self.api_base_url.to_string().trim_end_matches('/').into(),
            model: self.model.clone(),
        }
    }

    async fn send_chat_stream(
        &self,
        request: &ModelRequest,
        on_text_delta: &ModelTextDeltaHandler,
    ) -> Result<ModelResponse, ModelError> {
        // The native OpenAI shape is assembled here so the generic prompt
        // builder remains provider-neutral.  Streaming and non-streaming paths
        // converge on the same ModelResponse parser below.
        // OpenAI-compatible providers receive a conventional chat-completions
        // request, including native function schemas when tools are available.
        // The streaming parser below accumulates content and tool-call fragments
        // independently because providers may interleave both kinds of delta.
        let prompt = build_agent_prompt(request)?;
        let url = self
            .api_base_url
            .join("chat/completions")
            .map_err(|error| ModelError::InvalidUrl(error.to_string()))?;
        let mut body = json!({
            "model": self.model,
            "messages": [{"role": "user", "content": prompt}],
            "stream": true,
        });
        if !request.tools.is_empty() {
            body["tools"] = Value::Array(
                request
                    .tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": tool.name,
                                "description": tool.description,
                                "parameters": tool.parameters,
                            },
                        })
                    })
                    .collect(),
            );
        }

        let mut response = self
            .http
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(request_error)?;
        let status = response.status();
        if !status.is_success() {
            let bytes = response.bytes().await.map_err(request_error)?;
            return Err(map_openai_error(status, &bytes, &self.api_key));
        }

        let is_event_stream = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream"));
        if !is_event_stream {
            let bytes = response.bytes().await.map_err(request_error)?;
            return parse_openai_completion(&bytes);
        }

        let mut event_buffer = Vec::new();
        let mut streamed_reply = OpenAIStreamingReply::default();
        while let Some(chunk) = response.chunk().await.map_err(request_error)? {
            event_buffer.extend_from_slice(&chunk);
            while let Some((boundary, separator_length)) = sse_event_boundary(&event_buffer) {
                let event = event_buffer[..boundary].to_vec();
                event_buffer.drain(..boundary + separator_length);
                if consume_openai_sse_event(&event, &mut streamed_reply, on_text_delta)? {
                    return streamed_reply.finish();
                }
            }
        }
        if !event_buffer.is_empty() {
            consume_openai_sse_event(&event_buffer, &mut streamed_reply, on_text_delta)?;
        }
        streamed_reply.finish()
    }
}

#[async_trait]
impl ModelClient for OpenAICompatibleModelClient {
    // The trait method is intentionally thin: request shaping, transport
    // parsing, and provider-specific error mapping remain inside the client so
    // Agent only handles the normalized ModelResponse contract.
    async fn complete(
        &self,
        request: ModelRequest,
        on_text_delta: ModelTextDeltaHandler,
    ) -> Result<ModelResponse, ModelError> {
        self.send_chat_stream(&request, &on_text_delta).await
    }
}

#[async_trait]
impl ModelClient for CloudModelClient {
    // Cloud's text envelope is converted into the same final/tool response
    // shape as BYOK.  This keeps provider choice independent from Agent loop
    // behavior and confirmation handling.
    async fn complete(
        &self,
        request: ModelRequest,
        on_text_delta: ModelTextDeltaHandler,
    ) -> Result<ModelResponse, ModelError> {
        let prompt = build_agent_prompt(&request)?;
        let reply = self.send_chat_stream(prompt, &on_text_delta).await?;
        Ok(parse_agent_reply(&reply))
    }
}

fn cloud_http_client() -> Result<Client, ModelError> {
    // Timeouts are part of the model contract: a provider that stops sending
    // bytes must eventually return control to Agent cancellation and UI state.
    // Shared client limits bound connection setup and response wait time.  The
    // user agent is stable for server diagnostics without including account or
    // credential data.
    Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(90))
        .user_agent("StudyPulse-Desktop/1.0")
        .build()
        .map_err(request_error)
}

fn normalize_base_url(raw: &str) -> Result<Url, ModelError> {
    // URL normalization is a security boundary as well as convenience.  The
    // returned trailing slash makes join semantics stable, while rejecting
    // userinfo/query/fragment prevents credentials or hidden paths from leaking.
    // Provider URLs may be HTTPS or local HTTP only.  Userinfo, query, and
    // fragment components are rejected so path joining cannot reinterpret a
    // credential-bearing or ambiguous base URL.
    let raw = raw.trim().trim_end_matches('/');
    let url = Url::parse(raw).map_err(|error| ModelError::InvalidUrl(error.to_string()))?;
    if !matches!(url.scheme(), "https" | "http")
        || url.cannot_be_a_base()
        || url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ModelError::InvalidUrl(raw.into()));
    }
    if url.scheme() == "http" && !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
    {
        return Err(ModelError::InvalidUrl(
            "HTTP is only allowed for a local development server".into(),
        ));
    }
    let normalized = format!("{}/", url.as_str().trim_end_matches('/'));
    Url::parse(&normalized).map_err(|error| ModelError::InvalidUrl(error.to_string()))
}

fn validate_callback_base(callback_url: &str) -> Result<(), ModelError> {
    // Deep-link callbacks are fixed protocol coordinates, not arbitrary URLs.
    // Query and fragment rejection ensures login starts from a clean callback
    // base and tokens are accepted only from the returned query parameters.
    let url = Url::parse(callback_url).map_err(|_| ModelError::InvalidAuthCallback)?;
    if url.scheme() == "studypulse"
        && url.host_str() == Some("auth")
        && url.path() == "/callback"
        && url.query().is_none()
        && url.fragment().is_none()
    {
        Ok(())
    } else {
        Err(ModelError::InvalidAuthCallback)
    }
}

fn build_agent_prompt(request: &ModelRequest) -> Result<String, ModelError> {
    // The textual prompt is the common denominator for Cloud and BYOK.  It
    // includes schemas and transcript as data, but explicitly tells the model
    // that only the host can validate or execute the proposed operations.
    // Cloud and BYOK receive a textual envelope because their provider APIs do
    // not share a native transcript shape here.  The prompt explicitly states
    // that the host owns validation/execution, preserving the security boundary
    // even when a model is capable of emitting arbitrary JSON.
    let tools = serde_json::to_string(&request.tools).map_err(|_| ModelError::InvalidResponse)?;
    let messages =
        serde_json::to_string(&request.messages).map_err(|_| ModelError::InvalidResponse)?;
    let prompt = format!(
        r#"You are the StudyPulse Desktop learning Agent. You are an action-oriented assistant: inspect the user's Workspace with tools, then answer from the returned evidence. The host application, not you, validates and executes tools.

Active Agent mode: {mode}
Stages for this mode: {stages}

Return exactly one JSON object without a surrounding Markdown code fence.
Use ASCII JSON syntax only: keys and string values must use the plain double
quote character `"`; never use typographic “smart quotes”. Escape newlines,
backslashes, and embedded quotes inside string values.
To answer the user, return:
{{"type":"final","text":"your answer"}}
To request one or more tools, return:
{{"type":"tool_calls","calls":[{{"id":"unique-id","name":"tool_name","arguments":{{}}}}]}}

Rules:
- Treat the tool catalog as an API, not as documentation. When a tool can answer, verify, calculate, search, or create the requested result, call it instead of describing what you would do.
- If the request refers to Workspace files, notebook sources, tasks, or memory, call the relevant read tool before giving a factual answer. Never guess local data.
- If the request involves arithmetic, code, data transformation, a comparison that needs checking, or a visualization, call `code_execution` when it is listed. It runs Python locally after native user confirmation (or in the optional Docker backend when configured); never simulate an execution result.
- If the request needs current or external evidence, call `web_search` or `paper_search` when listed, and preserve the returned citations in the final answer.
- Use only listed tools and arguments matching their JSON schemas. Do not invent tool names, tool results, paths, or citations.
- Prefer the smallest useful read/execute sequence before any write. A write or execute operation is gated by native host permission; do not ask for permission in chat.
- After every tool result, either request the next necessary tool or return a final answer grounded in the result. Do not repeat an identical call after it has returned an error.
- Match the user's language.
- The final text field may use CommonMark or GitHub-flavored Markdown when it improves readability.

Available tools:
{tools}

Authoritative conversation and tool-result transcript:
{messages}"#,
        mode = request.mode.as_deref().unwrap_or("chat"),
        stages = if request.stages.is_empty() {
            "responding".to_owned()
        } else {
            request.stages.join(" -> ")
        }
    );
    if prompt.chars().count() > MAX_CLOUD_MESSAGE_CHARACTERS {
        // Reject before network I/O so an oversized transcript cannot consume
        // provider quota or create an unbounded request body.
        return Err(ModelError::RequestTooLarge);
    }
    Ok(prompt)
}

fn parse_agent_reply(reply: &str) -> ModelResponse {
    // Reply parsing is intentionally ordered from most specific to most
    // permissive.  A valid native envelope wins; compatibility extraction is
    // attempted only when a provider added harmless prose or a code fence.
    // Plain text is the final fallback and never becomes an executable call.
    // Providers sometimes wrap JSON in Markdown fences or explanatory text.
    // Try strict JSON, then the innermost object, then XML calls, and finally
    // expose plain text; tool-call syntax is never shown as an assistant answer
    // when a known call shape can be recovered.
    let trimmed = reply.trim();
    let candidate = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    let json_candidate = parse_json_candidate(candidate).or_else(|| {
        let start = candidate.find('{')?;
        let end = candidate.rfind('}')?;
        parse_json_candidate(&candidate[start..=end])
    });

    if let Some(value) = json_candidate {
        if let Some(response) = parse_tool_calls_value(&value) {
            return response;
        }
        if let Ok(AgentEnvelope::Final { text }) = serde_json::from_value::<AgentEnvelope>(value)
            && !text.trim().is_empty()
        {
            return ModelResponse {
                text_deltas: vec![text],
                tool_calls: Vec::new(),
                usage: None,
            };
        }
    }

    if let Some(response) = parse_xml_tool_calls(trimmed) {
        return response;
    }

    ModelResponse {
        text_deltas: vec![reply.to_owned()],
        tool_calls: Vec::new(),
        usage: None,
    }
}

fn parse_json_candidate(candidate: &str) -> Option<Value> {
    // Smart-quote normalization is a compatibility fallback for weaker models;
    // the primary path remains strict serde JSON so valid payloads are unchanged.
    serde_json::from_str::<Value>(candidate).ok().or_else(|| {
        normalize_smart_json(candidate)
            .and_then(|normalized| serde_json::from_str::<Value>(&normalized).ok())
    })
}

fn normalize_smart_json(input: &str) -> Option<String> {
    // This normalizer is a repair layer, not a second general-purpose parser.
    // It tracks only whether it is inside a typographic string and preserves
    // ordinary ASCII JSON untouched, limiting the chance of changing content.
    // Normalize typographic quotes and control characters only inside the
    // candidate string.  Returning None for an unterminated smart-quoted value
    // avoids turning an incomplete stream into a false tool call.
    let mut normalized = String::with_capacity(input.len());
    let mut in_smart_string = false;
    let mut characters = input.chars();

    while let Some(character) = characters.next() {
        if in_smart_string {
            match character {
                '”' => {
                    normalized.push('"');
                    in_smart_string = false;
                }
                '"' => normalized.push_str("\\\""),
                '\\' => {
                    normalized.push('\\');
                    if let Some(next) = characters.next() {
                        if matches!(next, '"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' | 'u') {
                            normalized.push(next);
                        } else {
                            normalized.push('\\');
                            normalized.push(next);
                        }
                    }
                }
                '\n' => normalized.push_str("\\n"),
                '\r' => normalized.push_str("\\r"),
                '\t' => normalized.push_str("\\t"),
                character if character.is_control() => {
                    normalized.push_str(&format!("\\u{:04x}", character as u32));
                }
                _ => normalized.push(character),
            }
        } else if character == '“' {
            normalized.push('"');
            in_smart_string = true;
        } else {
            normalized.push(character);
        }
    }

    (!in_smart_string).then_some(normalized)
}

/// Accept the StudyPulse envelope as well as the common shapes emitted by
/// providers that expose OpenAI-style or XML tool calls. The Cloud Worker
/// currently transports one text message, so this compatibility layer is the
/// equivalent of OpenCode's provider normalization boundary.
fn parse_tool_calls_value(value: &Value) -> Option<ModelResponse> {
    // Several providers use different nesting for function calls.  They are
    // accepted here because all candidates still pass through one decoder and
    // one call-id/name/arguments normalization path before reaching Agent.
    // Accept the native StudyPulse `calls` envelope and common `tool_calls`,
    // single-function, and array shapes.  All candidates converge through
    // decode_agent_call so name/argument defaults are applied consistently.
    let candidates = match value {
        Value::Array(values) => values.clone(),
        Value::Object(object) => {
            if let Some(calls) = object.get("calls").and_then(Value::as_array) {
                calls.clone()
            } else if let Some(calls) = object.get("tool_calls").and_then(Value::as_array) {
                calls.clone()
            } else if object.get("name").is_some()
                || object.get("tool").and_then(Value::as_str).is_some()
                || object.get("function").is_some()
            {
                vec![value.clone()]
            } else {
                return None;
            }
        }
        _ => return None,
    };
    let calls = candidates
        .iter()
        .filter_map(decode_agent_call)
        .collect::<Vec<_>>();
    model_response_from_calls(calls)
}

fn decode_agent_call(value: &Value) -> Option<AgentCall> {
    // The canonical serde decode is attempted first so the StudyPulse wire
    // shape remains the preferred contract.  Nested OpenAI-style functions are
    // only a compatibility fallback and cannot bypass the later tool schema.
    // The first decode preserves the canonical envelope.  The fallback handles
    // OpenAI-style nested function objects and stringified arguments without
    // assuming the provider supplied a call id.
    if let Ok(call) = serde_json::from_value::<AgentCall>(value.clone())
        && !call.name.trim().is_empty()
    {
        return Some(call);
    }

    let object = value.as_object()?;
    let function = object.get("function").and_then(Value::as_object);
    let name = object
        .get("name")
        .or_else(|| object.get("tool"))
        .or_else(|| function.and_then(|value| value.get("name")))
        .and_then(Value::as_str)?
        .trim();
    if name.is_empty() {
        return None;
    }
    let raw_arguments = object
        .get("arguments")
        .or_else(|| function.and_then(|value| value.get("arguments")));
    let arguments = match raw_arguments {
        Some(Value::String(text)) => {
            serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.clone()))
        }
        Some(value) => value.clone(),
        None => empty_arguments(),
    };
    Some(AgentCall {
        id: object.get("id").and_then(Value::as_str).map(str::to_owned),
        name: name.to_owned(),
        arguments,
    })
}

fn model_response_from_calls(calls: Vec<AgentCall>) -> Option<ModelResponse> {
    // This function is the executable boundary for parsed calls: empty names
    // are discarded and missing ids are filled locally so every Tool message
    // can be paired deterministically with the model request that produced it.
    // Missing ids are filled locally because the Agent tool-message protocol
    // requires a stable call id for the response that is sent back to the model.
    // Empty names are discarded rather than becoming executable unknown tools.
    let tool_calls = calls
        .into_iter()
        .filter(|call| !call.name.trim().is_empty())
        .map(|call| ModelToolCall {
            id: call
                .id
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
            name: call.name,
            arguments: call.arguments,
        })
        .collect::<Vec<_>>();
    (!tool_calls.is_empty()).then_some(ModelResponse {
        text_deltas: Vec::new(),
        tool_calls,
        usage: None,
    })
}

fn parse_xml_tool_calls(input: &str) -> Option<ModelResponse> {
    // XML support is intentionally narrow.  Only JSON bodies inside explicit
    // tool-call tags are decoded, so arbitrary XML is never treated as a model
    // action or copied into the normalized call list.
    // XML is accepted only as a narrow compatibility shape used by some local
    // providers; the body must still contain JSON that passes normal call
    // decoding before it can reach the host.
    let mut remaining = input;
    let mut calls = Vec::new();
    while let Some(start) = remaining.find("<tool_call>") {
        let body_start = start + "<tool_call>".len();
        let tail = &remaining[body_start..];
        let end = tail.find("</tool_call>")?;
        let body = tail[..end].trim();
        if let Ok(value) = serde_json::from_str::<Value>(body)
            && let Some(call) = decode_agent_call(&value)
        {
            calls.push(call);
        }
        remaining = &tail[end + "</tool_call>".len()..];
    }
    model_response_from_calls(calls)
}

#[derive(Default)]
struct StreamingAgentReply {
    raw: String,
    emitted_text: String,
}

impl StreamingAgentReply {
    // Cloud can stream a JSON object one character range at a time.  Keeping a
    // raw buffer lets the scanner distinguish a partial final text field from
    // a tool-call object, while `emitted_text` records the already delivered
    // prefix for suffix-only callbacks.
    // The raw buffer is retained because Cloud streams partial JSON envelopes.
    // Only a completed `type: final` payload is safe to expose; emitted_text
    // makes callback delivery incremental without leaking tool-call JSON.
    fn push(&mut self, delta: &str, on_text_delta: &ModelTextDeltaHandler) {
        self.raw.push_str(delta);
        let Some(text) = partial_final_text(&self.raw) else {
            return;
        };
        let Some(new_text) = text.strip_prefix(&self.emitted_text) else {
            return;
        };
        if !new_text.is_empty() {
            on_text_delta(new_text.to_owned());
            self.emitted_text = text;
        }
    }

    fn finish(self) -> Result<String, ModelError> {
        // An empty stream is a protocol failure rather than an empty answer, so
        // callers can surface an invalid provider response distinctly.
        (!self.raw.trim().is_empty())
            .then_some(self.raw)
            .ok_or(ModelError::InvalidResponse)
    }
}

#[derive(Default)]
struct OpenAIStreamingReply {
    content: StreamingAgentReply,
    tool_calls: BTreeMap<usize, OpenAIToolCallAccumulator>,
    usage: Option<ModelUsage>,
}

// OpenAI-compatible tool calls arrive as fragments keyed by `index`.  Keeping
// a separate accumulator for each index allows content and multiple calls to
// be interleaved without changing their eventual order.
#[derive(Default)]
struct OpenAIToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
}

impl OpenAIStreamingReply {
    // OpenAI-compatible streams carry independent content and tool-call
    // fragments.  The accumulator keeps them separate until the provider sends
    // a terminal marker, then converges both paths into ModelResponse.
    // Prefer accumulated native tool calls when present; otherwise finish the
    // content stream through the Cloud-compatible envelope parser.  This is the
    // single convergence point for streaming and non-streaming semantics.
    fn finish(self) -> Result<ModelResponse, ModelError> {
        let usage = self.usage;
        if !self.tool_calls.is_empty() {
            let calls = self
                .tool_calls
                .into_values()
                .map(|call| {
                    json!({
                        "id": call.id,
                        "function": {
                            "name": call.name,
                            "arguments": call.arguments,
                        },
                    })
                })
                .collect::<Vec<_>>();
            let response = calls
                .iter()
                .filter_map(decode_agent_call)
                .collect::<Vec<_>>();
            let mut normalized =
                model_response_from_calls(response).ok_or(ModelError::InvalidResponse)?;
            normalized.usage = usage;
            return Ok(normalized);
        }

        let content = self.content.finish()?;
        let mut normalized = parse_agent_reply(&content);
        normalized.usage = usage;
        Ok(normalized)
    }
}

fn sse_event_boundary(bytes: &[u8]) -> Option<(usize, usize)> {
    // Both LF and CRLF separators are common in SSE implementations.  Returning
    // the separator length lets the caller drain exactly one complete event.
    bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| (index, 2))
        .or_else(|| {
            bytes
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|index| (index, 4))
        })
}

fn consume_sse_event(
    event: &[u8],
    reply: &mut StreamingAgentReply,
    on_text_delta: &ModelTextDeltaHandler,
) -> Result<bool, ModelError> {
    // SSE framing is transport syntax, not model content.  This parser removes
    // `data:` prefixes and terminal markers before the JSON response reaches
    // the provider-normalization layer.
    // Cloud SSE carries one or more `data:` lines per event.  Empty events are
    // ignored, `[DONE]` ends the stream, and only recognized final text reaches
    // the UI callback through StreamingAgentReply.
    let event = std::str::from_utf8(event).map_err(|_| ModelError::InvalidResponse)?;
    let data = event
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() {
        return Ok(false);
    }
    if data.trim() == "[DONE]" {
        return Ok(true);
    }
    let value: Value = serde_json::from_str(&data).map_err(|_| ModelError::InvalidResponse)?;
    if let Some(delta) = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("delta"))
        .and_then(|delta| delta.get("content"))
        .and_then(Value::as_str)
    {
        reply.push(delta, on_text_delta);
    }
    Ok(false)
}

fn consume_openai_sse_event(
    event: &[u8],
    reply: &mut OpenAIStreamingReply,
    on_text_delta: &ModelTextDeltaHandler,
) -> Result<bool, ModelError> {
    // Tool-call fragments are merged by provider-supplied index.  The index is
    // the only stable ordering signal when name and arguments arrive in
    // different chunks, so callers must not append fragments by arrival alone.
    // The OpenAI-compatible parser handles provider errors, content deltas, and
    // native tool-call fragments in one event.  Index-based accumulation is
    // necessary because id/name/arguments may arrive in different chunks.
    let event = std::str::from_utf8(event).map_err(|_| ModelError::InvalidResponse)?;
    let data = event
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() {
        return Ok(false);
    }
    if data.trim() == "[DONE]" {
        return Ok(true);
    }

    let value: Value = serde_json::from_str(&data).map_err(|_| ModelError::InvalidResponse)?;
    if value.get("error").is_some() {
        let message = decode_error_message(data.as_bytes())
            .unwrap_or_else(|| "OpenAI-compatible provider returned an error".into());
        return Err(ModelError::Request(message));
    }
    if let Some(usage) = parse_model_usage(value.get("usage")) {
        reply.usage = Some(usage);
    }
    let Some(choice) = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
    else {
        return Ok(false);
    };
    let delta = choice.get("delta").cloned().unwrap_or_else(|| json!({}));
    if let Some(content) = delta.get("content").and_then(Value::as_str) {
        reply.content.push(content, on_text_delta);
    }
    if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
        for tool_call in tool_calls {
            let index = tool_call
                .get("index")
                .and_then(Value::as_u64)
                .ok_or(ModelError::InvalidResponse)? as usize;
            let accumulator = reply.tool_calls.entry(index).or_default();
            if let Some(id) = tool_call.get("id").and_then(Value::as_str) {
                accumulator.id.push_str(id);
            }
            let Some(function) = tool_call.get("function") else {
                continue;
            };
            if let Some(name) = function.get("name").and_then(Value::as_str) {
                accumulator.name.push_str(name);
            }
            if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                accumulator.arguments.push_str(arguments);
            }
        }
    }
    Ok(false)
}

fn parse_openai_completion(bytes: &[u8]) -> Result<ModelResponse, ModelError> {
    // Non-streaming responses use the same call decoder as SSE fragments.  This
    // keeps provider choice from changing whether a tool call is executable.
    // Non-streaming OpenAI responses use the same native tool-call shape.  A
    // plain message is normalized through parse_agent_reply so final envelopes
    // and provider-specific plain text remain compatible.
    let value: Value = serde_json::from_slice(bytes).map_err(|_| ModelError::InvalidResponse)?;
    let usage = parse_model_usage(value.get("usage"));
    if value.get("error").is_some() {
        let message = decode_error_message(bytes)
            .unwrap_or_else(|| "OpenAI-compatible provider returned an error".into());
        return Err(ModelError::Request(message));
    }
    let message = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .ok_or(ModelError::InvalidResponse)?;
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        let response = calls
            .iter()
            .filter_map(decode_agent_call)
            .collect::<Vec<_>>();
        if let Some(mut response) = model_response_from_calls(response) {
            response.usage = usage;
            return Ok(response);
        }
    }
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .filter(|content| !content.trim().is_empty())
        .ok_or(ModelError::InvalidResponse)?;
    let mut response = parse_agent_reply(content);
    response.usage = usage;
    Ok(response)
}

fn parse_model_usage(value: Option<&Value>) -> Option<ModelUsage> {
    let value = value?.as_object()?;
    let prompt_tokens = value.get("prompt_tokens").and_then(Value::as_u64)?;
    let completion_tokens = value
        .get("completion_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let total_tokens = value
        .get("total_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| prompt_tokens.saturating_add(completion_tokens));
    Some(ModelUsage {
        prompt_tokens,
        completion_tokens,
        total_tokens,
        estimated: false,
    })
}

fn partial_final_text(input: &str) -> Option<String> {
    // Incremental display must wait until the type field says `final`.  This
    // small rule is what keeps a partial `tool_calls` object from flashing as
    // assistant prose while the provider is still transmitting it.
    // Streaming text is exposed only after the response type is fully known to
    // be `final`.  This prevents a partial tool-call object from appearing as
    // chat text while it is still being assembled.
    let (response_type, type_complete) = find_json_string_field(input, "type")?;
    if !type_complete || response_type != "final" {
        return None;
    }
    find_json_string_field(input, "text").map(|(text, _)| text)
}

fn find_json_string_field(input: &str, field: &str) -> Option<(String, bool)> {
    // The scanner deliberately understands only string fields and escapes.  A
    // full document parse would reject the incomplete prefixes that are normal
    // during SSE, while a broad permissive parser could expose malformed data.
    // This small scanner intentionally tolerates an incomplete closing quote:
    // callers use the boolean to distinguish a complete field from a prefix
    // that should wait for another SSE delta.
    let bytes = input.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'"' {
            cursor += 1;
            continue;
        }
        let key = decode_json_string(input, cursor)?;
        let key_end = key.end?;
        cursor = key_end;
        if key.value != field {
            continue;
        }
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b':') {
            continue;
        }
        cursor += 1;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'"') {
            return None;
        }
        let value = decode_json_string(input, cursor)?;
        return Some((value.value, value.end.is_some()));
    }
    None
}

struct DecodedJsonString {
    value: String,
    end: Option<usize>,
}

fn decode_json_string(input: &str, opening_quote: usize) -> Option<DecodedJsonString> {
    // Returning `end: None` for an unfinished quote is useful information, not
    // an error: the streaming caller can retain the decoded prefix and wait for
    // the next chunk without losing already received Unicode text.
    // The scanner covers the JSON escapes needed while a provider response is
    // still incomplete.  It does not attempt to parse an entire document, which
    // keeps incremental text delivery independent from serde's all-or-nothing
    // document parser.
    let bytes = input.as_bytes();
    if bytes.get(opening_quote) != Some(&b'"') {
        return None;
    }
    let mut value = String::new();
    let mut cursor = opening_quote + 1;
    let mut segment_start = cursor;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'"' => {
                value.push_str(&input[segment_start..cursor]);
                return Some(DecodedJsonString {
                    value,
                    end: Some(cursor + 1),
                });
            }
            b'\\' => {
                value.push_str(&input[segment_start..cursor]);
                cursor += 1;
                let escaped = *bytes.get(cursor)?;
                match escaped {
                    b'"' => value.push('"'),
                    b'\\' => value.push('\\'),
                    b'/' => value.push('/'),
                    b'b' => value.push('\u{0008}'),
                    b'f' => value.push('\u{000C}'),
                    b'n' => value.push('\n'),
                    b'r' => value.push('\r'),
                    b't' => value.push('\t'),
                    b'u' => {
                        let (character, next_cursor) = decode_unicode_escape(bytes, cursor)?;
                        value.push(character);
                        cursor = next_cursor - 1;
                    }
                    _ => return None,
                }
                cursor += 1;
                segment_start = cursor;
            }
            byte if byte < 0x20 => return None,
            _ => cursor += 1,
        }
    }
    value.push_str(&input[segment_start..]);
    Some(DecodedJsonString { value, end: None })
}

fn decode_unicode_escape(bytes: &[u8], u_index: usize) -> Option<(char, usize)> {
    // Decode BMP escapes and surrogate pairs without accepting an invalid scalar
    // value.  Provider text is thus reconstructed as valid Rust characters even
    // when the escape crosses an SSE chunk boundary.
    let high = decode_hex_quad(bytes.get(u_index + 1..u_index + 5)?)?;
    let next = u_index + 5;
    if !(0xD800..=0xDBFF).contains(&high) {
        return char::from_u32(u32::from(high)).map(|character| (character, next));
    }
    if bytes.get(next..next + 2)? != b"\\u" {
        return None;
    }
    let low = decode_hex_quad(bytes.get(next + 2..next + 6)?)?;
    if !(0xDC00..=0xDFFF).contains(&low) {
        return None;
    }
    let scalar = 0x10000 + ((u32::from(high) - 0xD800) << 10) + (u32::from(low) - 0xDC00);
    char::from_u32(scalar).map(|character| (character, next + 6))
}

fn decode_hex_quad(bytes: &[u8]) -> Option<u16> {
    // Keep hexadecimal parsing narrow and allocation-free for the incremental
    // scanner; callers decide whether the resulting code unit is a valid scalar.
    (bytes.len() == 4).then_some(())?;
    bytes.iter().try_fold(0_u16, |value, byte| {
        let digit = (*byte as char).to_digit(16)?;
        Some((value << 4) | digit as u16)
    })
}

fn request_error(error: reqwest::Error) -> ModelError {
    // Transport failures deliberately remain a generic request category; the
    // provider-specific status mapping below handles semantic HTTP failures.
    ModelError::Request(error.to_string())
}

fn map_cloud_error(status: StatusCode, body: &[u8]) -> ModelError {
    // HTTP status is mapped before any provider message is shown.  The category
    // gives the UI a recovery action, while the bounded decoded message adds
    // context without exposing raw response bodies or credentials.
    // Cloud status codes map to user-actionable categories while preserving a
    // bounded provider message.  Unauthorized means refresh/login, not a bad
    // request, so it remains distinguishable from generic transport failure.
    let message = decode_error_message(body).unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
    match status {
        StatusCode::UNAUTHORIZED => ModelError::SessionExpired,
        StatusCode::FORBIDDEN => ModelError::AccessDenied(message),
        StatusCode::TOO_MANY_REQUESTS => ModelError::QuotaExceeded(message),
        StatusCode::PAYLOAD_TOO_LARGE => ModelError::RequestTooLarge,
        _ => ModelError::Request(message),
    }
}

fn map_openai_error(status: StatusCode, body: &[u8], api_key: &str) -> ModelError {
    // BYOK errors receive an extra redaction pass because third-party gateways
    // occasionally echo authorization material in their JSON error payload.
    // The sanitized message is the only value allowed to cross this boundary.
    // BYOK providers are less uniform, so status mapping is shared but the
    // message is sanitized against the configured secret before it leaves this
    // crate.  No error path should echo the API key into UI or logs.
    let message = decode_error_message(body).unwrap_or_else(|| {
        format!(
            "OpenAI-compatible provider returned HTTP {}",
            status.as_u16()
        )
    });
    let message = if api_key.is_empty() {
        message
    } else {
        message.replace(api_key, "[redacted]")
    };
    match status {
        StatusCode::UNAUTHORIZED => {
            ModelError::Request(format!("BYOK API key was rejected: {message}"))
        }
        StatusCode::FORBIDDEN => ModelError::AccessDenied(message),
        StatusCode::TOO_MANY_REQUESTS => ModelError::QuotaExceeded(message),
        StatusCode::PAYLOAD_TOO_LARGE => ModelError::RequestTooLarge,
        _ => ModelError::Request(message),
    }
}

fn decode_error_message(body: &[u8]) -> Option<String> {
    // Support string and object error envelopes, preferring a human message and
    // falling back to a provider code when no message exists.
    let value: Value = serde_json::from_slice(body).ok()?;
    match value.get("error")? {
        Value::String(message) => Some(message.clone()),
        Value::Object(error) => error
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| error.get("code").and_then(Value::as_str).map(str::to_owned)),
        _ => None,
    }
}

fn tool_call(name: &str, arguments: Value) -> ModelResponse {
    // The mock and tests use the same response constructor as a normalized
    // provider call, including a locally generated id for tool-message pairing.
    ModelResponse {
        text_deltas: Vec::new(),
        tool_calls: vec![ModelToolCall {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            arguments,
        }],
        usage: None,
    }
}

#[derive(Debug, Deserialize)]
struct ProfileEnvelope {
    success: bool,
    data: Option<ProfileData>,
}

// Cloud response envelopes intentionally use optional fields because server
// versions have added membership and plan data independently.  Required
// authentication tokens are validated after deserialization, not assumed here.
#[derive(Debug, Deserialize)]
struct ProfileData {
    email: Option<String>,
    role: Option<String>,
    membership: Option<MembershipData>,
    plan: Option<PlanData>,
}

#[derive(Debug, Deserialize)]
struct MembershipData {
    #[serde(rename = "type")]
    membership_type: Option<String>,
    expires_at: Option<String>,
    effective_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PlanData {
    name: Option<String>,
    available_models: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct RefreshData {
    access_token: Option<String>,
    refresh_token: Option<String>,
    session_token: Option<String>,
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RefreshEnvelope {
    access_token: Option<String>,
    refresh_token: Option<String>,
    data: Option<RefreshData>,
}

#[derive(Debug, Deserialize)]
struct ChatEnvelope {
    data: Option<ChatData>,
}

#[derive(Debug, Deserialize)]
struct ChatData {
    reply: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AgentEnvelope {
    Final { text: String },
}

// AgentCall is the internal compatibility shape before conversion to the public
// ModelToolCall.  An absent id is allowed at this stage and filled at the
// normalization boundary so legacy providers remain usable.
#[derive(Debug, Deserialize)]
struct AgentCall {
    id: Option<String>,
    name: String,
    #[serde(default = "empty_arguments")]
    arguments: Value,
}

// These envelope structs intentionally mirror only fields consumed by the
// client.  Unknown server additions remain harmless, while required token
// prefixes and response presence are checked explicitly after decoding.

fn empty_arguments() -> Value {
    // Serde uses this default for providers that omit an arguments member; the
    // tool registry still applies the target tool's strict schema afterward.
    json!({})
}

// Provider tests stay network-free and focus on normalization: callback URL
// validation, token prefixes, redaction, framing, and compatibility envelopes.
// Transport integration belongs to the caller's environment, while these
// deterministic cases protect the protocol boundary on every platform.
#[cfg(test)]
mod tests {
    // Tests cover wire compatibility, incremental filtering, auth validation,
    // HTTP error classification, and native OpenAI tool-call accumulation.  The
    // fixtures intentionally avoid real credentials and external network calls.
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        thread,
        time::Duration,
    };

    use super::*;

    #[test]
    fn login_url_encodes_the_registered_callback() {
        // Auth URL generation must preserve the exact deep-link callback so a
        // successful login returns to the desktop instead of another target.
        let url = CloudModelClient::login_url("studypulse://auth/callback").unwrap();
        let parsed = Url::parse(&url).unwrap();
        assert_eq!(parsed.host_str(), Some("auth.chenkai.space"));
        assert_eq!(
            parsed
                .query_pairs()
                .find(|(key, _)| key == "return_to")
                .map(|(_, value)| value.into_owned()),
            Some("studypulse://auth/callback".into())
        );
    }

    #[test]
    fn auth_callback_requires_both_expected_token_types() {
        // Both token prefixes are required before the host can establish a
        // session; malformed values never become CloudAuthTokens.
        let tokens = CloudModelClient::parse_auth_callback(
            "studypulse://auth/callback?access_token=sp_sess_abc&refresh_token=sp_refresh_xyz",
        )
        .unwrap();
        assert_eq!(tokens.access_token, "sp_sess_abc");
        assert_eq!(tokens.refresh_token, "sp_refresh_xyz");
        assert!(
            CloudModelClient::parse_auth_callback(
                "studypulse://auth/callback?access_token=plain&refresh_token=sp_refresh_xyz"
            )
            .is_err()
        );
    }

    #[test]
    fn tool_envelope_is_converted_to_model_calls() {
        // The canonical Agent envelope is normalized before the runtime sees it,
        // keeping provider JSON separate from ToolRegistry execution.
        let response = parse_agent_reply(
            r#"{"type":"tool_calls","calls":[{"name":"get_tasks","arguments":{}}]}"#,
        );
        assert!(response.text_deltas.is_empty());
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "get_tasks");
        assert!(!response.tool_calls[0].id.is_empty());
    }

    #[test]
    fn common_provider_tool_call_shapes_are_normalized() {
        // OpenAI-style nested functions and single-call shapes converge on the
        // same ModelToolCall representation.
        let openai = parse_agent_reply(
            r#"{"tool_calls":[{"id":"call-1","function":{"name":"code_execution","arguments":"{\"language\":\"python\",\"code\":\"print(2 + 2)\"}"}}]}"#,
        );
        assert_eq!(openai.tool_calls[0].name, "code_execution");
        assert_eq!(openai.tool_calls[0].arguments["language"], "python");

        let xml =
            parse_agent_reply(r#"<tool_call>{"name":"get_tasks","arguments":{}}</tool_call>"#);
        assert_eq!(xml.tool_calls[0].name, "get_tasks");
    }

    #[test]
    fn smart_quoted_tool_envelopes_are_normalized_without_exposing_them_as_text() {
        // Smart quotes are a compatibility fallback, but a recovered call must
        // remain a call rather than leaking its JSON into assistant text.
        let response = parse_agent_reply(
            r#"{“type”:“tool_calls”,“calls”:[{“id”:“call-smart”,“name”:“code_execution”,“arguments”:{“language”:“python”,“code”:“print(\"ok\")\n”}}]}"#,
        );
        assert!(response.text_deltas.is_empty());
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "code_execution");
        assert_eq!(response.tool_calls[0].arguments["language"], "python");
        assert_eq!(response.tool_calls[0].arguments["code"], "print(\"ok\")\n");
    }

    #[test]
    fn final_and_plain_responses_are_presented_as_text() {
        // Final envelopes become text deltas; an unrecognized provider reply is
        // still preserved as plain text rather than discarded.
        assert_eq!(
            parse_agent_reply(r#"{"type":"final","text":"Ready"}"#).text_deltas,
            vec!["Ready"]
        );
        assert_eq!(
            parse_agent_reply("ordinary reply").text_deltas,
            vec!["ordinary reply"]
        );
    }

    #[test]
    fn final_markdown_is_emitted_incrementally_without_the_json_envelope() {
        // Streaming callbacks expose only final text after its type is known,
        // proving the UI never renders partial protocol JSON.
        let received = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let received_by_handler = Arc::clone(&received);
        let handler: ModelTextDeltaHandler = Arc::new(move |delta| {
            received_by_handler.lock().unwrap().push(delta);
        });
        let mut reply = StreamingAgentReply::default();

        reply.push(r##"{"type":"final","text":"# Plan\n- "##, &handler);
        reply.push(r##"Review **algebra**\n- Finish \u2705"}"##, &handler);

        assert_eq!(
            received.lock().unwrap().concat(),
            "# Plan\n- Review **algebra**\n- Finish ✅"
        );
        assert_eq!(
            parse_agent_reply(&reply.finish().unwrap()).text_deltas,
            vec!["# Plan\n- Review **algebra**\n- Finish ✅"]
        );
    }

    #[test]
    fn tool_call_json_is_not_exposed_as_streamed_chat_text() {
        // Tool-call streams are intentionally silent on the text callback; the
        // completed response carries the call for host-side execution.
        let received = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let received_by_handler = Arc::clone(&received);
        let handler: ModelTextDeltaHandler = Arc::new(move |delta| {
            received_by_handler.lock().unwrap().push(delta);
        });
        let mut reply = StreamingAgentReply::default();

        reply.push(
            r#"{"type":"tool_calls","calls":[{"name":"get_tasks","arguments":{}}]}"#,
            &handler,
        );

        assert!(received.lock().unwrap().is_empty());
    }

    #[test]
    fn cloud_errors_support_both_error_envelopes() {
        // Error classification accepts string and object provider bodies,
        // preserving actionable categories without one server shape.
        assert_eq!(
            decode_error_message(br#"{"error":"Daily request limit exceeded"}"#).as_deref(),
            Some("Daily request limit exceeded")
        );
        assert_eq!(
            decode_error_message(
                br#"{"success":false,"error":{"code":"SESSION_EXPIRED","message":"Expired"}}"#
            )
            .as_deref(),
            Some("Expired")
        );
    }

    #[test]
    fn insecure_non_local_base_url_is_rejected() {
        // Plain HTTP is allowed only for loopback development endpoints; remote
        // providers must use HTTPS before credentials are sent.
        assert!(normalize_base_url("http://spapi.chenkai.space").is_err());
        assert!(normalize_base_url("http://127.0.0.1:8787").is_ok());
    }

    #[tokio::test]
    async fn cloud_profile_uses_bearer_auth_and_plan_models() {
        // Profile requests prove bearer placement and optional plan/model
        // decoding without exposing the token in the returned profile.
        let (base_url, request) = one_shot_server(
            StatusCode::OK,
            json!({
                "success": true,
                "data": {
                    "email": "student@example.com",
                    "role": "user",
                    "membership": {
                        "type": "free",
                        "effective_type": "pro",
                        "expires_at": "2026-08-30T00:00:00Z"
                    },
                    "plan": {
                        "name": "Pro",
                        "available_models": ["MiniMax-M3"]
                    }
                }
            }),
        );
        let client = CloudModelClient::new(&base_url, "sp_sess_test".into(), None).unwrap();

        let profile = client.profile().await.unwrap();

        assert_eq!(profile.email, "student@example.com");
        assert_eq!(profile.membership_type, "pro");
        assert_eq!(profile.available_models, ["MiniMax-M3"]);
        let request = request.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(request.starts_with("GET /user/profile HTTP/1.1"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer sp_sess_test")
        );
    }

    #[tokio::test]
    async fn cloud_completion_sends_agent_context_and_decodes_tool_call() {
        // Cloud completion includes mode/stage/history/tool context and converts
        // the worker's tool envelope into a host call.
        let reply = json!({
            "type": "tool_calls",
            "calls": [{"id": "call-1", "name": "get_tasks", "arguments": {}}]
        })
        .to_string();
        let (base_url, request) = one_shot_sse_server(&[reply]);
        let client =
            CloudModelClient::new(&base_url, "sp_sess_test".into(), Some("MiniMax-M3".into()))
                .unwrap();

        let response = client
            .complete(
                ModelRequest {
                    messages: vec![ChatMessage::User {
                        content: "Plan chemistry".into(),
                    }],
                    tools: vec![ModelToolDefinition {
                        name: "get_tasks".into(),
                        description: "Read tasks".into(),
                        parameters: json!({"type": "object"}),
                        permission: Some("read".into()),
                    }],
                    mode: None,
                    stages: Vec::new(),
                },
                Arc::new(|_| {}),
            )
            .await
            .unwrap();

        assert_eq!(response.tool_calls[0].name, "get_tasks");
        let request = request.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(request.starts_with("POST /v1/chat HTTP/1.1"));
        assert!(request.contains("Plan chemistry"));
        assert!(request.contains("get_tasks"));
        assert!(request.contains(r#""model":"MiniMax-M3""#));
        assert!(request.contains(r#""stream":true"#));
    }

    #[tokio::test]
    async fn cloud_completion_streams_final_markdown_deltas() {
        // Cloud SSE callbacks are incremental final text, while the returned
        // response retains the complete answer for the next Agent turn.
        let (base_url, _request) = one_shot_sse_server(&[
            r##"{"type":"final","text":"# Plan\n"##.into(),
            r##"- Review algebra"}"##.into(),
        ]);
        let client = CloudModelClient::new(&base_url, "sp_sess_test".into(), None).unwrap();
        let received = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let received_by_handler = Arc::clone(&received);

        let response = client
            .complete(
                ModelRequest {
                    messages: vec![ChatMessage::User {
                        content: "Plan algebra".into(),
                    }],
                    tools: Vec::new(),
                    mode: None,
                    stages: Vec::new(),
                },
                Arc::new(move |delta| {
                    received_by_handler.lock().unwrap().push(delta);
                }),
            )
            .await
            .unwrap();

        assert_eq!(
            received.lock().unwrap().concat(),
            "# Plan\n- Review algebra"
        );
        assert_eq!(response.text_deltas, ["# Plan\n- Review algebra"]);
    }

    #[tokio::test]
    async fn byok_completion_uses_openai_chat_completions_and_bearer_auth() {
        // BYOK uses the OpenAI-compatible request path and bearer key without
        // changing the provider-neutral ModelClient interface.
        let reply = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": r#"{"type":"tool_calls","calls":[{"name":"get_tasks","arguments":{}}]}"#
                }
            }]
        });
        let (base_url, request) = one_shot_server(StatusCode::OK, reply);
        let client = OpenAICompatibleModelClient::new(
            &format!("{base_url}/v1"),
            "sk-test-key".into(),
            "gpt-test".into(),
        )
        .unwrap();

        let response = client
            .complete(
                ModelRequest {
                    messages: vec![ChatMessage::User {
                        content: "Plan chemistry".into(),
                    }],
                    tools: vec![ModelToolDefinition {
                        name: "get_tasks".into(),
                        description: "Read tasks".into(),
                        parameters: json!({"type": "object"}),
                        permission: Some("read".into()),
                    }],
                    mode: None,
                    stages: Vec::new(),
                },
                Arc::new(|_| {}),
            )
            .await
            .unwrap();

        assert_eq!(response.tool_calls[0].name, "get_tasks");
        let request = request.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer sk-test-key")
        );
        assert!(request.contains(r#""model":"gpt-test"#));
        assert!(request.contains(r#""stream":true"#));
        assert!(request.contains(r#""type":"function"#));
    }

    #[tokio::test]
    async fn byok_completion_streams_openai_final_content() {
        // Native OpenAI content deltas stream as text and finish as the same
        // normalized response shape used by non-streaming calls.
        let first = "{\"type\":\"final\",\"text\":\"# Plan\\n".to_owned();
        let second = "- Review algebra\"}".to_owned();
        let (base_url, _request) = one_shot_sse_server(&[first, second]);
        let client =
            OpenAICompatibleModelClient::new(&base_url, "sk-test-key".into(), "gpt-test".into())
                .unwrap();
        let received = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let received_by_handler = Arc::clone(&received);

        let response = client
            .complete(
                ModelRequest {
                    messages: vec![ChatMessage::User {
                        content: "Plan algebra".into(),
                    }],
                    tools: Vec::new(),
                    mode: None,
                    stages: Vec::new(),
                },
                Arc::new(move |delta| {
                    received_by_handler.lock().unwrap().push(delta);
                }),
            )
            .await
            .unwrap();

        assert_eq!(
            received.lock().unwrap().concat(),
            "# Plan\n- Review algebra"
        );
        assert_eq!(response.text_deltas, ["# Plan\n- Review algebra"]);
    }

    #[test]
    fn byok_non_stream_response_decodes_native_openai_tool_calls() {
        // Non-streaming native tool calls are decoded without requiring the
        // textual StudyPulse envelope.
        let response = parse_openai_completion(
            br#"{
                "usage": {"prompt_tokens": 4, "completion_tokens": 3, "total_tokens": 7},
                "choices": [{
                    "message": {
                        "tool_calls": [{
                            "id": "call-1",
                            "function": {
                                "name": "get_tasks",
                                "arguments": "{}"
                            }
                        }
                        ]
                    }
                }]
            }"#,
        )
        .unwrap();

        assert_eq!(response.tool_calls[0].id, "call-1");
        assert_eq!(response.tool_calls[0].name, "get_tasks");
        assert_eq!(
            response.usage.as_ref().map(|usage| usage.total_tokens),
            Some(7)
        );
    }

    #[test]
    fn byok_streaming_response_accumulates_native_openai_tool_calls() {
        // Fragmented id/name/argument fields are accumulated by tool-call index
        // before the runtime receives a complete call.
        let handler: ModelTextDeltaHandler = Arc::new(|_| {});
        let mut reply = OpenAIStreamingReply::default();
        consume_openai_sse_event(
            br#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-1","function":{"name":"get_","arguments":"{"}}]}}]}

"#,
            &mut reply,
            &handler,
        )
        .unwrap();
        consume_openai_sse_event(
            br#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"tasks","arguments":"}"}}]}}]}

"#,
            &mut reply,
            &handler,
        )
        .unwrap();

        let response = reply.finish().unwrap();
        assert_eq!(response.tool_calls[0].id, "call-1");
        assert_eq!(response.tool_calls[0].name, "get_tasks");
        assert_eq!(response.tool_calls[0].arguments, json!({}));
    }

    fn one_shot_server(status: StatusCode, body: Value) -> (String, mpsc::Receiver<String>) {
        // Test servers capture one request and return one deterministic body,
        // keeping transport tests local and credential-free.
        one_shot_response(status, "application/json", body.to_string())
    }

    fn one_shot_sse_server(deltas: &[String]) -> (String, mpsc::Receiver<String>) {
        // SSE fixtures emit explicit event boundaries so parser tests cover
        // chunking rather than relying on a real network stream's packet shape.
        let mut body = deltas
            .iter()
            .map(|delta| {
                format!(
                    "data: {}\n\n",
                    json!({"choices": [{"delta": {"content": delta}}]})
                )
            })
            .collect::<String>();
        body.push_str("data: [DONE]\n\n");
        one_shot_response(StatusCode::OK, "text/event-stream", body)
    }

    fn one_shot_response(
        status: StatusCode,
        content_type: &'static str,
        body: String,
    ) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let read = stream.read(&mut buffer).unwrap_or(0);
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if request_is_complete(&request) {
                    break;
                }
            }
            sender
                .send(String::from_utf8_lossy(&request).into_owned())
                .unwrap();
            let response = format!(
                "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("OK"),
                content_type,
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        (format!("http://{address}"), receiver)
    }

    fn request_is_complete(request: &[u8]) -> bool {
        let Some(header_end) = request.windows(4).position(|value| value == b"\r\n\r\n") else {
            return false;
        };
        let header = String::from_utf8_lossy(&request[..header_end]);
        let content_length = header
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        request.len() >= header_end + 4 + content_length
    }
}
