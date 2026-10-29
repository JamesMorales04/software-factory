//! Explicit provider adapter boundary.
//!
//! The provider catalog names *connections*; this module states which adapter
//! Core can actually apply to each connection. An adapter is not a credential
//! or an authorization. It only answers whether a concrete transport exists for
//! a role:
//!
//! - `opencode` connections use the OpenAI-compatible chat-completions adapter,
//!   which Core also uses for the bounded Lead assessment and semantic review.
//! - `codex` connections use the Codex App Server adapter inside the Worker.
//!   This build has **no** one-shot chat adapter for the App Server, so a Codex
//!   connection can execute the implementer role but not assessment or semantic
//!   review. Pretending the App Server were an OpenAI gateway would fabricate a
//!   capability that does not exist.
//!
//! Model-catalog policy is per connection, not per adapter. `opencode-go` and
//! `opencode` share this adapter but publish their catalogs at different URLs,
//! so [`crate::provider_catalog`] owns that decision.
//!
//! Decision mechanism: `DETERMINISTIC`.

use crate::provider_catalog;

/// A transport Core implements for one provider connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderAdapter {
    /// OpenAI-compatible `POST /chat/completions`.
    OpenAiChatCompletions,
    /// Codex App Server JSON-RPC over stdio, driven by the Worker.
    CodexAppServer,
}

impl ProviderAdapter {
    /// Map the stored `provider_settings.kind` to an implemented adapter. An
    /// unknown kind is not coerced to the nearest adapter.
    pub fn from_kind(kind: &str) -> Option<Self> {
        match kind {
            "opencode" => Some(Self::OpenAiChatCompletions),
            "codex" => Some(Self::CodexAppServer),
            _ => None,
        }
    }

    /// Map a catalog provider connection to its adapter.
    pub fn from_provider(provider: &str) -> Option<Self> {
        provider_catalog::entry(provider).and_then(|entry| Self::from_kind(entry.kind))
    }

    pub fn kind(self) -> &'static str {
        match self {
            Self::OpenAiChatCompletions => "opencode",
            Self::CodexAppServer => "codex",
        }
    }

    /// Whether Core can execute bounded one-shot model calls (Lead assessment,
    /// semantic review, evaluation) through this adapter.
    pub fn supports_chat_completions(self) -> bool {
        matches!(self, Self::OpenAiChatCompletions)
    }

    /// Whether the Worker can execute an implementation session through this
    /// adapter.
    pub fn supports_provider_session(self) -> bool {
        matches!(self, Self::OpenAiChatCompletions | Self::CodexAppServer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_implemented_transports_are_recognised() {
        assert_eq!(
            ProviderAdapter::from_kind("opencode"),
            Some(ProviderAdapter::OpenAiChatCompletions)
        );
        assert_eq!(
            ProviderAdapter::from_kind("codex"),
            Some(ProviderAdapter::CodexAppServer)
        );
        assert_eq!(ProviderAdapter::from_kind("fixture"), None);
        assert_eq!(ProviderAdapter::from_provider("unknown"), None);
        assert_eq!(
            ProviderAdapter::from_provider("opencode-go"),
            Some(ProviderAdapter::OpenAiChatCompletions)
        );
    }

    #[test]
    fn codex_is_honest_about_one_shot_roles() {
        let codex = ProviderAdapter::CodexAppServer;
        assert!(!codex.supports_chat_completions());
        assert!(codex.supports_provider_session());
        let opencode = ProviderAdapter::OpenAiChatCompletions;
        assert!(opencode.supports_chat_completions());
        assert!(opencode.supports_provider_session());
    }
}
