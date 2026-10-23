use std::{env, path::Path};

use factory_native::runtime;

#[tokio::main]
async fn main() {
    let provider = env::args().nth(1).unwrap_or_default();
    let workspace = env::args().nth(2).unwrap_or_else(|| ".".into());
    let result = match provider.as_str() {
        "codex" => runtime::probe_codex(Path::new(&workspace)).await,
        "opencode" => runtime::probe_opencode(Path::new(&workspace)).await,
        _ => {
            eprintln!("usage: factory-provider-probe <codex|opencode> [workspace]");
            std::process::exit(2);
        }
    };
    match result {
        Ok(result) => println!(
            "{}",
            serde_json::to_string_pretty(&result).expect("serializable result")
        ),
        Err(error) => {
            eprintln!("probe failed: {error}");
            std::process::exit(1);
        }
    }
}
