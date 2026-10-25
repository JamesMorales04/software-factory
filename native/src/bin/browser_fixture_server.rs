//! Disposable application test server for the managed-browser integration
//! tests. It is not a product capability: it serves one static page and obeys
//! the HOST/PORT/FACTORY_TEST_ENDPOINT contract.
//!
//! Test-only modes:
//! - `--child`: sleep so a parent fixture can prove process-tree cleanup.
//! - `--spawn-child`: spawn the `--child` descendant before listening.
//! - `--unready`: stay alive without binding to exercise startup timeout.
//! - `--exit-early`: exit immediately to exercise early-exit diagnosis.

use std::time::Duration;

use axum::{Router, response::Html, routing::get};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|value| value == "--child") {
        tokio::time::sleep(Duration::from_secs(300)).await;
        return Ok(());
    }
    if args.iter().any(|value| value == "--unready") {
        tokio::time::sleep(Duration::from_secs(300)).await;
        return Ok(());
    }
    if args.iter().any(|value| value == "--exit-early") {
        return Err("fixture exited before readiness".into());
    }
    if args.iter().any(|value| value == "--spawn-child") {
        let executable = std::env::current_exe()?;
        tokio::process::Command::new(executable)
            .arg("--child")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
    }
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    if host != "127.0.0.1" {
        return Err("fixture requires HOST=127.0.0.1".into());
    }
    let port = std::env::var("PORT")
        .map_err(|_| "PORT is required")?
        .parse::<u16>()?;
    let endpoint =
        std::env::var("FACTORY_TEST_ENDPOINT").unwrap_or_else(|_| format!("http://{host}:{port}"));
    let app = Router::new()
        .route(
            "/",
            get(|| async {
                Html(
                    "<!doctype html><html><head><title>Factory browser fixture</title></head>\
                     <body><main id=\"factory-browser-fixture\">Factory browser fixture ready</main>\
                     </body></html>",
                )
            }),
        )
        .route("/health", get(|| async { "ok" }));
    let listener = TcpListener::bind((host.as_str(), port)).await?;
    println!("factory-browser-fixture listening on {endpoint}");
    axum::serve(listener, app).await?;
    Ok(())
}
