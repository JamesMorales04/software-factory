//! Real bubblewrap provider boundary tests. These are candidate-bound local
//! observations, not independent isolation certification.

use std::time::Duration;

use factory_native::provider_sandbox::{ProviderSandbox, ProviderSandboxConfig};

fn bwrap_available() -> bool {
    std::process::Command::new("bwrap")
        .args([
            "--unshare-all",
            "--die-with-parent",
            "--ro-bind",
            "/usr",
            "/usr",
            "--ro-bind-try",
            "/bin",
            "/bin",
            "--ro-bind-try",
            "/lib",
            "/lib",
            "--ro-bind-try",
            "/lib64",
            "/lib64",
            "--proc",
            "/proc",
            "/bin/true",
        ])
        .output()
        .is_ok_and(|output| output.status.success())
}

#[tokio::test]
async fn sandbox_broker_requires_capability_and_rejects_unbounded_paths() {
    if !bwrap_available() {
        return;
    }
    let session_dir = std::env::temp_dir().join(format!("ps-{}", uuid::Uuid::new_v4().simple()));
    let sandbox = ProviderSandbox::start(ProviderSandboxConfig {
        binary: std::path::PathBuf::from(env!("CARGO_BIN_EXE_factory-fixture-provider")),
        session_dir: session_dir.clone(),
        args: Vec::new(),
        ready_timeout: Duration::from_secs(10),
    })
    .await
    .expect("start sandboxed fixture provider");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let base = sandbox.base_url();
    let password = sandbox.password();

    assert_eq!(
        client
            .get(format!("{base}/global/health"))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .get(format!("{base}/global/health"))
            .basic_auth("opencode", Some("wrong-password"))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .get(format!("{base}/v1/workspaces"))
            .basic_auth("opencode", Some(password))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::FORBIDDEN
    );
    let health: serde_json::Value = client
        .get(format!("{base}/global/health"))
        .basic_auth("opencode", Some(password))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["healthy"], true);

    let probe = std::fs::read_to_string(session_dir.join("probe.txt")).unwrap();
    assert!(probe.contains("host_home_exists=false"), "{probe}");
    assert!(probe.contains("podman_socket_exists=false"), "{probe}");
    assert!(probe.contains("network_egress=false"), "{probe}");
    let environment = std::fs::read_to_string(session_dir.join("environment.txt")).unwrap();
    for forbidden in [
        "FACTORY_PROVIDER_PASSWORD",
        "FACTORY_OPERATOR_TOKEN",
        "FACTORY_WORKER_BOOTSTRAP_TOKEN",
        "FACTORY_CORE_URL",
    ] {
        assert!(
            !environment.contains(forbidden),
            "sandbox environment leaked {forbidden}: {environment}"
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&session_dir)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
    }
    sandbox.stop().await.unwrap();
    let _ = std::fs::remove_dir_all(session_dir);
}
