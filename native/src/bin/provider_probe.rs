use std::{env, path::Path};

use factory_native::runtime;
use serde::Serialize;

fn print_result<T: Serialize, E: std::fmt::Display>(result: Result<T, E>) {
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

#[tokio::main]
async fn main() {
    let mut args = env::args().skip(1);
    let provider = args.next().unwrap_or_default();
    match provider.as_str() {
        "codex" => {
            let workspace = args.next().unwrap_or_else(|| ".".into());
            print_result(runtime::probe_codex(Path::new(&workspace)).await);
        }
        "opencode" => {
            let workspace = args.next().unwrap_or_else(|| ".".into());
            print_result(runtime::probe_opencode(Path::new(&workspace)).await);
        }
        "codex-reconcile" => {
            let thread_id = args.next().unwrap_or_default();
            if thread_id.is_empty() || thread_id.len() > 128 {
                eprintln!("codex-reconcile requires a thread ID of 1–128 characters");
                std::process::exit(2);
            }
            let workspace = args.next().unwrap_or_else(|| ".".into());
            print_result(runtime::reconcile_codex_thread(Path::new(&workspace), &thread_id).await);
        }
        _ => {
            eprintln!(
                "usage: factory-provider-probe <codex|opencode> [workspace] | codex-reconcile <thread-id> [workspace]"
            );
            std::process::exit(2);
        }
    }
}
