use factory_infinity_experiment::subscription::{
    BillingObservation, CodexAuthMode, FakeCodexAppServer, FakeSubscriptionProvider,
    FundingViolation, OwnerAttestation, PreflightDecision, RouteKind, SubscriptionGuard,
    feasibility_matrix,
};

fn fully_attested() -> OwnerAttestation {
    OwnerAttestation {
        opencode_go_subscription_active: true,
        opencode_go_extra_usage_disabled: true,
        opencode_go_credit_fallback_disabled: true,
        no_api_key_billing_credentials: true,
        chatgpt_subscription_active: true,
        codex_app_server_authorized: true,
        workspace_confinement_verified: true,
        live_test_authorized: true,
    }
}

#[test]
fn matrix_separates_completion_adapters_from_coding_agents() {
    let routes = feasibility_matrix();
    let opencode = routes
        .iter()
        .find(|route| route.id == "opencode-go-deepseek-v4.1-flash")
        .expect("opencode route");
    assert_eq!(opencode.kind, RouteKind::CompletionAdapter);
    assert!(opencode.streaming);
    assert!(opencode.tool_call_compatible);
    assert!(!opencode.allowed_without_further_authorization);
    assert!(!opencode.sources.is_empty());

    let codex = routes
        .iter()
        .find(|route| route.id == "codex-app-server-chatgpt")
        .expect("codex route");
    assert_eq!(codex.kind, RouteKind::ExternalCodingAgent);
    assert!(
        !codex.tool_call_compatible,
        "an agent is not a completion adapter"
    );

    for forbidden in [
        "openai-chatgpt-oauth-completion",
        "openai-api-key",
        "aws-bedrock",
    ] {
        let route = routes
            .iter()
            .find(|route| route.id == forbidden)
            .expect("forbidden route");
        assert_eq!(route.kind, RouteKind::Forbidden, "{forbidden}");
        assert!(!route.allowed_without_further_authorization);
    }
    let json = serde_json::to_string_pretty(&routes).expect("matrix json");
    assert!(json.contains("no_overage_controls"));
}

#[test]
fn default_attestation_fails_closed() {
    let guard = SubscriptionGuard::new(OwnerAttestation::default());
    for route in [
        "opencode-go-deepseek-v4.1-flash",
        "codex-app-server-chatgpt",
        "openai-api-key",
        "aws-bedrock",
        "unknown-route",
    ] {
        let decision = guard.preflight(route);
        assert!(
            matches!(decision, PreflightDecision::Deny(_)),
            "{route} must be denied"
        );
    }
}

#[test]
fn opencode_go_requires_every_no_overage_attestation() {
    let mut attestation = fully_attested();
    let guard = SubscriptionGuard::new(attestation.clone());
    assert!(
        guard
            .preflight("opencode-go-deepseek-v4.1-flash")
            .is_allowed()
    );

    attestation.opencode_go_extra_usage_disabled = false;
    let guard = SubscriptionGuard::new(attestation.clone());
    let decision = guard.preflight("opencode-go-deepseek-v4.1-flash");
    assert!(!decision.is_allowed());
    assert!(format!("{decision:?}").contains("Extra Usage"));

    attestation.opencode_go_extra_usage_disabled = true;
    attestation.live_test_authorized = false;
    let guard = SubscriptionGuard::new(attestation);
    assert!(
        !guard
            .preflight("opencode-go-deepseek-v4.1-flash")
            .is_allowed()
    );
}

#[tokio::test]
async fn fake_completion_adapter_refuses_non_subscription_billing() {
    use infinity_provider_protocol::{CompletionRequest, ModelProvider};

    use futures_util::StreamExt;
    let go = FakeSubscriptionProvider::new("opencode-go-deepseek-v4.1-flash", "go", 0.0);
    let mut stream = go
        .invoke_model("deepseek-v4.1-flash", CompletionRequest::default())
        .await
        .expect("fake completion");
    while stream.next().await.is_some() {}
    let observation = go.last_observation().expect("observation");
    let guard = SubscriptionGuard::new(fully_attested());
    assert!(
        guard
            .observe("opencode-go-deepseek-v4.1-flash", &observation)
            .is_ok()
    );

    let balance = FakeSubscriptionProvider::new("opencode-go-deepseek-v4.1-flash", "balance", 0.02);
    let mut stream = balance
        .invoke_model("deepseek-v4.1-flash", CompletionRequest::default())
        .await
        .expect("fake completion");
    while stream.next().await.is_some() {}
    let observation = balance.last_observation().expect("observation");
    assert_eq!(
        guard.observe("opencode-go-deepseek-v4.1-flash", &observation),
        Err(FundingViolation::ChargedAmount)
    );

    let byok = FakeSubscriptionProvider::new("opencode-go-deepseek-v4.1-flash", "byok", 0.0);
    let mut stream = byok
        .invoke_model("deepseek-v4.1-flash", CompletionRequest::default())
        .await
        .expect("fake completion");
    while stream.next().await.is_some() {}
    let observation = byok.last_observation().expect("observation");
    assert!(matches!(
        guard.observe("opencode-go-deepseek-v4.1-flash", &observation),
        Err(FundingViolation::NonSubscriptionBillingSource(_))
    ));

    let missing = BillingObservation {
        billing_source: None,
        cost_usd: None,
        allowance_usd: None,
    };
    assert_eq!(
        guard.observe("opencode-go-deepseek-v4.1-flash", &missing),
        Err(FundingViolation::MissingBillingMetadata)
    );
}

#[test]
fn codex_app_server_is_an_agent_not_a_completion_provider() {
    let mut api_key = FakeCodexAppServer::new(CodexAuthMode::ApiKey);
    assert!(api_key.initialize().is_err());
    assert!(api_key.start_thread_and_turn("review").is_err());

    let mut chatgpt = FakeCodexAppServer::new(CodexAuthMode::ChatGptSubscription);
    chatgpt.initialize().expect("chatgpt auth");
    let events = chatgpt
        .start_thread_and_turn("review the candidate")
        .expect("turn");
    assert!(
        events
            .iter()
            .any(|event| format!("{event:?}").contains("ThreadStarted"))
    );
    assert!(
        events
            .iter()
            .any(|event| format!("{event:?}").contains("TurnCompleted"))
    );
    assert_eq!(chatgpt.events().len(), 4);
    assert_eq!(
        chatgpt.interrupt(),
        factory_infinity_experiment::subscription::CodexTurnEvent::Interrupted
    );
}
