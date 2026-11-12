//! Development CLI for the Core tool-gateway library.
//!
//! This is not part of the installed Factory runtime and does not grant
//! authority. It lets a developer exercise the curated catalog against a
//! supplied `FACTORY_TOOL_GATEWAY_CONFIG` before the Worker integration lands.

use std::env;

use factory_native::tool_gateway::{GatewayRole, ToolGateway};
use serde_json::Value;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args().skip(1);
    let role = arguments
        .next()
        .ok_or("usage: factory-tool-gateway <ROLE> <TOOL> [JSON_ARGUMENTS]")?;
    let tool = arguments
        .next()
        .ok_or("usage: factory-tool-gateway <ROLE> <TOOL> [JSON_ARGUMENTS]")?;
    let raw = arguments.next().unwrap_or_else(|| "{}".into());
    let role = GatewayRole::parse(&role).ok_or("unknown gateway role")?;
    let arguments: Value = serde_json::from_str(&raw)?;
    let gateway = ToolGateway::from_env()?.ok_or(
        "tool gateway is disabled; set FACTORY_TOOL_GATEWAY_CONFIG to the validated catalog",
    )?;
    let outcome = gateway.call(role, &tool, arguments).await?;
    println!("{}", serde_json::to_string_pretty(&outcome)?);
    Ok(())
}
