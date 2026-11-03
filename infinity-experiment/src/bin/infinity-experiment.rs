//! Command-line harness for the Infinity-first Factory experiment.
//!
//! All runs are deterministic and offline. The process-exit crash modes exist
//! so durability can be demonstrated across real process boundaries rather
//! than simulated in one address space.

use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};
use factory_infinity_experiment::effects::CrashPolicy;
use factory_infinity_experiment::runtime::{
    JourneyReport, PrototypeConfig, resume_journey, run_journey,
};

#[derive(Parser)]
#[command(name = "infinity-experiment")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the deterministic Director/implementer/reviewer journey.
    Run(RunArgs),
    /// Run and terminate the process at a durable effect boundary.
    Start(StartArgs),
    /// Recover a previously crashed run on a new process.
    Resume(RunArgs),
    /// Write the subscription-route feasibility matrix (deterministic data).
    Subscription(SubscriptionArgs),
}

#[derive(clap::Args)]
struct RunArgs {
    #[arg(long)]
    root: PathBuf,
    #[arg(long)]
    run_id: String,
    #[arg(long, default_value_t = 40)]
    effect_delay_ms: u64,
    #[arg(long)]
    out: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
struct StartArgs {
    #[arg(long)]
    root: PathBuf,
    #[arg(long)]
    run_id: String,
    /// "any" crashes after the first applied effect; otherwise a task id.
    #[arg(long, default_value = "review")]
    crash_after_effect: String,
    #[arg(long, default_value_t = 40)]
    effect_delay_ms: u64,
}

#[derive(clap::Args)]
struct SubscriptionArgs {
    #[arg(long)]
    out: Option<PathBuf>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .try_init();
    let cli = Cli::parse();
    let local = tokio::task::LocalSet::new();
    let code = local
        .run_until(async move {
            match cli.command {
                Command::Run(args) => run(args).await,
                Command::Start(args) => start(args).await,
                Command::Resume(args) => resume(args).await,
                Command::Subscription(args) => subscription(args),
            }
        })
        .await;
    std::process::exit(code);
}

async fn run(args: RunArgs) -> i32 {
    let mut config = PrototypeConfig::fixture(&args.root, &args.run_id);
    config.effect_delay = Duration::from_millis(args.effect_delay_ms);
    match run_journey(&config).await {
        Ok(report) => emit(report, args.out.as_ref(), args.json),
        Err(error) => {
            eprintln!("journey failed: {error}");
            1
        }
    }
}

async fn start(args: StartArgs) -> i32 {
    let mut config = PrototypeConfig::fixture(&args.root, &args.run_id);
    config.effect_delay = Duration::from_millis(args.effect_delay_ms);
    config.crash = if args.crash_after_effect == "any" {
        CrashPolicy::AfterEffect
    } else {
        CrashPolicy::AfterEffectNamed(args.crash_after_effect)
    };
    match run_journey(&config).await {
        Ok(report) => emit(report, None, false),
        Err(error) => {
            eprintln!("journey failed before crash boundary: {error}");
            1
        }
    }
}

async fn resume(args: RunArgs) -> i32 {
    let mut config = PrototypeConfig::fixture(&args.root, &args.run_id);
    config.effect_delay = Duration::from_millis(args.effect_delay_ms);
    match resume_journey(&config).await {
        Ok(report) => emit(report, args.out.as_ref(), args.json),
        Err(error) => {
            eprintln!("resume failed: {error}");
            1
        }
    }
}

fn emit(report: JourneyReport, out: Option<&PathBuf>, json: bool) -> i32 {
    let encoded = serde_json::to_string_pretty(&report).expect("serialize report");
    if let Some(path) = out {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, &encoded).expect("write report");
    }
    if json {
        println!("{encoded}");
    } else {
        let candidate = report
            .candidate
            .as_ref()
            .map(|candidate| candidate.candidate_sha256.as_str())
            .unwrap_or("none");
        println!(
            "run={} success={} candidate={} effects={} callbacks={} events={} wall_ms={} peak_rss_kib={}",
            report.run_id,
            report.success,
            candidate,
            report.effects.len(),
            report.callbacks.len(),
            report.metrics.event_count,
            report.metrics.wall_ms,
            report.metrics.peak_rss_kib,
        );
        if let Some(recovery) = &report.recovery {
            println!(
                "recovery: recovered={} ambiguous={} duplicate_callbacks={}",
                recovery.recovered_completed.len(),
                recovery.ambiguous.len(),
                recovery.duplicate_callbacks
            );
        }
    }
    if report.success { 0 } else { 2 }
}

fn subscription(args: SubscriptionArgs) -> i32 {
    let matrix = factory_infinity_experiment::subscription::feasibility_matrix();
    let encoded = serde_json::to_string_pretty(&matrix).expect("serialize matrix");
    match args.out {
        Some(path) => {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(path, &encoded).expect("write matrix");
            println!("wrote subscription feasibility matrix");
        }
        None => println!("{encoded}"),
    }
    0
}
