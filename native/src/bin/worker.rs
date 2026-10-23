use std::{env, time::Duration};

use serde_json::json;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = env::var("FACTORY_WORKER_TOKEN").map_err(|_| "FACTORY_WORKER_TOKEN required")?;
    if token.is_empty() {
        return Err("FACTORY_WORKER_TOKEN must not be empty".into());
    }
    let core = env::var("FACTORY_CORE_URL").unwrap_or_else(|_| "http://127.0.0.1:8765".into());
    let core = validated_core_url(&core)?;
    let worker_id = Uuid::new_v4().to_string();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let response = client
        .post(core.join("worker/hello")?)
        .bearer_auth(token)
        .json(&json!({"worker_id":worker_id}))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!("Core refused Worker handshake: {}", response.status()).into());
    }
    let body: serde_json::Value = response.json().await?;
    if body
        .get("authenticated")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        return Err("Core did not confirm authentication".into());
    }
    println!("Worker authenticated; dispatch_supported=false");
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
