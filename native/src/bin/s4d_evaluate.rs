use std::{env, fs, process::ExitCode};

use factory_native::evaluation::{
    EvaluationCorpus, EvaluationScoreSheet, ModelEvaluationConfig, baseline, compare, evaluate,
    execute_model_evaluation, frozen_corpus,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("factory-s4d-evaluate: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice() {
        [command] if command == "corpus" => {
            let corpus = frozen_corpus()?;
            println!("{}", serde_json::to_string_pretty(&corpus)?);
        }
        [command] if command == "baseline" => {
            let corpus = read_corpus(None)?;
            let values: Vec<_> = corpus
                .fixtures
                .iter()
                .map(|fixture| {
                    baseline(&factory_native::evaluation::BaselineInput {
                        request: fixture.request.clone(),
                        evidence: fixture.evidence.clone(),
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&values)?);
        }
        [command, corpus_path] if command == "validate" => {
            let corpus = read_corpus(Some(corpus_path))?;
            println!("{} fixtures in {}", corpus.fixtures.len(), corpus.id);
        }
        [command, corpus_path, model_path, baseline_path] if command == "compare" => {
            let corpus = read_corpus(Some(corpus_path))?;
            let model: EvaluationScoreSheet = read_json(model_path)?;
            let baseline_sheet: EvaluationScoreSheet = read_json(baseline_path)?;
            let model_result = evaluate(&corpus, &model)?;
            let baseline_result = evaluate(&corpus, &baseline_sheet)?;
            let comparison = compare(&corpus, &model_result, &baseline_result)?;
            println!("{}", serde_json::to_string_pretty(&comparison)?);
            if !comparison.pass {
                return Err("CAP-014 thresholds were not met".into());
            }
        }
        [command, core_url, evaluation_id, output_path] if command == "execute" => {
            let token = factory_native::secrets::required("FACTORY_OPERATOR_TOKEN")
                .map_err(|_| "FACTORY_OPERATOR_TOKEN required for CAP-014 execution")?;
            let corpus = frozen_corpus()?;
            let evidence = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(execute_model_evaluation(
                    &corpus,
                    &ModelEvaluationConfig {
                        core_url: core_url.clone(),
                        operator_token: token,
                        evaluation_id: evaluation_id.clone(),
                        timeout: std::time::Duration::from_secs(180),
                    },
                ))?;
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            let mut file = options.open(output_path)?;
            use std::io::Write;
            file.write_all(serde_json::to_string_pretty(&evidence)?.as_bytes())?;
            file.write_all(b"\n")?;
            println!(
                "{} evaluation fixtures recorded in {output_path}",
                evidence.fixtures.len()
            );
            if evidence.status != "COMPLETE" {
                return Err(
                    format!("CAP-014 execution did not complete: {}", evidence.error).into(),
                );
            }
        }
        _ => {
            return Err(
                "usage: factory-s4d-evaluate <corpus|baseline|validate CORPUS|compare CORPUS MODEL BASELINE|execute CORE_URL EVALUATION_ID OUTPUT>"
                    .into(),
            );
        }
    }
    Ok(())
}

fn read_corpus(path: Option<&str>) -> Result<EvaluationCorpus, Box<dyn std::error::Error>> {
    match path {
        Some(path) => {
            let bytes = fs::read(path)?;
            let corpus: EvaluationCorpus = serde_json::from_slice(&bytes)?;
            if corpus.id != factory_native::evaluation::CORPUS_ID {
                return Err("corpus identity does not match CAP-014".into());
            }
            Ok(corpus)
        }
        None => Ok(frozen_corpus()?),
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
