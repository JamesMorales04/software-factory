use std::{env, time::Duration};

use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bootstrap_token = env::var("FACTORY_WORKER_BOOTSTRAP_TOKEN")
        .map_err(|_| "FACTORY_WORKER_BOOTSTRAP_TOKEN required")?;
    let worker_id = env::var("FACTORY_WORKER_ID").map_err(|_| "FACTORY_WORKER_ID required")?;
    let registration_key = env::var("FACTORY_WORKER_REGISTRATION_KEY")
        .map_err(|_| "FACTORY_WORKER_REGISTRATION_KEY required")?;
    if bootstrap_token.is_empty() || worker_id.trim().is_empty() || registration_key.is_empty() {
        return Err(
            "Worker bootstrap token, identity and registration key must not be empty".into(),
        );
    }
    let core = env::var("FACTORY_CORE_URL").unwrap_or_else(|_| "http://127.0.0.1:8765".into());
    let core = validated_core_url(&core)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let response = client
        .post(core.join("worker/register")?)
        .bearer_auth(bootstrap_token)
        .json(&json!({"worker_id":worker_id,"registration_key":registration_key}))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!("Core refused Worker registration: {}", response.status()).into());
    }
    let body: serde_json::Value = response.json().await?;
    let session_token = body
        .get("session_token")
        .and_then(serde_json::Value::as_str)
        .ok_or("Core did not return a Worker session credential")?;
    if body.get("worker_id").and_then(serde_json::Value::as_str) != Some(worker_id.as_str()) {
        return Err("Core returned a different Worker identity".into());
    }
    // The credential stays in memory and is never printed or logged. Each
    // restart repeats registration with the same key; Core rotates the prior
    // credential and returns a replacement.
    let claim = client
        .post(core.join("worker/claim")?)
        .bearer_auth(session_token)
        .json(&json!({"worker_id":worker_id}))
        .send()
        .await?;
    if !claim.status().is_success() {
        return Err(format!("Core refused Worker session: {}", claim.status()).into());
    }
    let claim: serde_json::Value = claim.json().await?;
    if claim
        .get("dispatch_supported")
        .and_then(serde_json::Value::as_bool)
        != Some(false)
        || !claim.get("lease").is_some_and(serde_json::Value::is_null)
    {
        return Err("Core returned an assignment this Worker cannot execute".into());
    }
    println!("Worker session authenticated; dispatch_supported=false");
    Ok(())
}

fn validated_core_url(raw: &str) -> Result<reqwest::Url, Box<dyn std::error::Error>> {
    let url = reqwest::Url::parse(raw)?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("127.0.0.1" | "localhost"))
        || url.port().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("FACTORY_CORE_URL must be a loopback HTTP origin with explicit port".into());
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::validated_core_url;

    #[test]
    fn rejects_userinfo_host_bypass() {
        assert!(validated_core_url("http://127.0.0.1:x@attacker.example").is_err());
        assert!(validated_core_url("http://localhost:8765").is_ok());
    }
}
