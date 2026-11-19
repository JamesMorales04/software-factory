//! Opt-in real identity-facts self-test.
//!
//! This test is ignored by the deterministic suite. When explicitly run with
//! `FACTORY_IDENTITY_FACTS_REAL_TEST=1` it uses ordinary rootless Podman and an
//! existing pinned environment-builder image to:
//!
//! 1. seed a disposable store volume with a real closure from that image,
//! 2. run the **same** broker producer (`observe_identity_facts_with`) with its
//!    fixed `nix path-info`/`nix derivation show` argv,
//! 3. sign the envelope with the environment bearer,
//! 4. verify it through the actual Core client (`verify_identity_facts`), and
//! 5. consume it through the actual SQLite store, refusing a replay.
//!
//! It never starts a paid model, the product stack or a host Nix install.

use std::future::Future;
use std::pin::Pin;
use std::process::Command;

use factory_native::db::Database;
use factory_native::planning::identity_facts::{
    BoundIdentityFacts, IDENTITY_FACTS_CONTEXT, IDENTITY_FACTS_SCHEMA_VERSION,
    IdentityFactsChallenge, IdentityFactsEnvelope, IdentityFactsMode, IdentityFactsReceipt,
    OperationRoot, closure_manifest_sha256, sign,
};
use factory_native::runtime_broker::{
    IdentityFactsCommandRunner, ResourceOwner, RuntimeBrokerClient, observe_identity_facts_with,
};

struct HostPodman {
    podman: String,
}

impl HostPodman {
    fn output(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        let output = Command::new(&self.podman)
            .args(args)
            .output()
            .map_err(|error| format!("podman {:?}: {error}", args.first()))?;
        if !output.status.success() {
            return Err(format!(
                "podman {:?} failed: {}",
                args.first(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(output.stdout)
    }
}

impl IdentityFactsCommandRunner for HostPodman {
    fn run_identity_command<'a>(
        &'a self,
        args: Vec<String>,
        operation: &'a str,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>> {
        Box::pin(async move {
            let output = Command::new(&self.podman)
                .args(&args)
                .output()
                .map_err(|error| format!("{operation} could not start: {error}"))?;
            if !output.status.success() {
                return Err(format!(
                    "{operation} failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
            if output.stdout.len() > limit {
                return Err(format!("{operation} exceeded its output bound"));
            }
            Ok(output.stdout)
        })
    }
}

struct DisposableVolumes {
    podman: String,
    volumes: Vec<String>,
}

impl Drop for DisposableVolumes {
    fn drop(&mut self) {
        for volume in &self.volumes {
            let _ = Command::new(&self.podman)
                .args(["volume", "rm", "--force", volume])
                .output();
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

#[tokio::test]
#[ignore = "requires real rootless Podman and the pinned environment-builder image"]
async fn real_identity_facts_producer_core_verifier_and_sqlite_replay() {
    if std::env::var("FACTORY_IDENTITY_FACTS_REAL_TEST")
        .ok()
        .as_deref()
        != Some("1")
    {
        eprintln!("FACTORY_IDENTITY_FACTS_REAL_TEST is not 1; skipping the real self-test");
        return;
    }
    let podman = std::env::var("FACTORY_PODMAN_BIN").unwrap_or_else(|_| "podman".into());
    let image = std::env::var("FACTORY_IDENTITY_FACTS_REAL_IMAGE")
        .expect("FACTORY_IDENTITY_FACTS_REAL_IMAGE must name an existing builder image");
    let id = uuid::Uuid::new_v4().simple().to_string();
    let store_volume = format!("factory-idf-selftest-store-{}", &id[..24]);
    let cache_volume = format!("factory-idf-selftest-cache-{}", &id[..24]);
    let _volumes = DisposableVolumes {
        podman: podman.clone(),
        volumes: vec![store_volume.clone(), cache_volume.clone()],
    };
    let runner = HostPodman {
        podman: podman.clone(),
    };
    runner
        .output(&["volume", "create", &store_volume])
        .expect("create the disposable store volume");
    runner
        .output(&["volume", "create", &cache_volume])
        .expect("create the disposable cache volume");

    // Use a real store path from the pinned image and copy only its closure and
    // derivations into the disposable store volume.
    let root = String::from_utf8_lossy(
        &runner
            .output(&[
                "run",
                "--rm",
                "--network=none",
                "--read-only",
                "--entrypoint=/bin/sh",
                &image,
                "-c",
                "p=$(readlink -f /bin/sh); printf '%s' \"${p%/bin/*}\"",
            ])
            .expect("resolve a real store path from the pinned image"),
    )
    .trim()
    .to_owned();
    assert!(
        factory_native::runtime_broker::is_store_output_path(&root),
        "the fixture root must be a real store path: {root}"
    );
    let seed_script = format!(
        r#"set -eu
root={root}
nixpath=$(readlink -f "$(command -v nix)")
nixroot=${{nixpath%/bin/*}}
readlinkpath=$(readlink -f "$(command -v readlink)")
readlinkroot=${{readlinkpath%/bin/*}}
paths=$(nix path-info --json --recursive --offline "$root" "$nixroot" "$readlinkroot" | jq -r 'keys[]')
drvs=$(for p in $paths; do d=$(nix-store -q --deriver "$p" 2>/dev/null || true); if [ -n "$d" ] && [ "$d" != unknown-deriver ]; then printf '%s\n' "$d"; fi; done | sort -u)
tar -C /nix -cf - $(for p in $paths $drvs; do printf '%s\n' "${{p#/nix/}}"; done) var/nix/db | tar -C /target -xf -
mkdir -p /target/var/nix/profiles/default/bin
ln -sfn "$nixroot/bin/nix" /target/var/nix/profiles/default/bin/nix
ln -sfn "$readlinkroot/bin/readlink" /target/var/nix/profiles/default/bin/readlink
chown -R 10001:10001 /target
"#
    );
    runner
        .output(&[
            "run",
            "--rm",
            "--network=none",
            "--user=0:0",
            "--entrypoint=/bin/sh",
            &format!("--volume={store_volume}:/target:rw"),
            &image,
            "-c",
            &seed_script,
        ])
        .expect("seed the disposable store volume from the pinned image");
    runner
        .output(&[
            "run",
            "--rm",
            "--network=none",
            "--user=0:0",
            "--entrypoint=/bin/sh",
            &format!("--volume={cache_volume}:/cache:rw"),
            &image,
            "-c",
            &format!("ln -sfn {root} /cache/profile"),
        ])
        .expect("write the realised profile into the disposable cache volume");

    let plan_sha256 = sha256_hex(
        serde_json::json!({
            "source": "PROJECT_COMMITTED",
            "packages": [],
            "lock": "devenv.lock",
        })
        .to_string()
        .as_bytes(),
    );
    let token = format!("identity-facts-selftest-token-{}", &id[..24]);
    let challenge = IdentityFactsChallenge {
        operation_id: format!("selftest-operation-{}", &id[..24]),
        runtime_id: format!("sha256:{}", sha256_hex(image.as_bytes())),
        nonce: format!("selftest-nonce-{}", &id[..24]),
        pin: "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d".into(),
        plan_sha256,
        spec_sha256: sha256_hex(b"selftest-spec"),
        mode: IdentityFactsMode::Enumerate,
        root: OperationRoot::Operation,
        questions: Vec::new(),
    };
    let manifest =
        observe_identity_facts_with(&runner, &image, &cache_volume, &store_volume, &challenge)
            .await
            .expect("the broker producer observes the real closure");
    assert_eq!(manifest.observed_root, root);
    assert!(!manifest.facts.is_empty());
    assert!(!manifest.truncated);
    assert_eq!(manifest.fact_count, manifest.facts.len());
    assert_eq!(
        manifest.closure_manifest_sha256,
        closure_manifest_sha256(&manifest.facts)
    );
    for fact in &manifest.facts {
        if fact.deriver.is_none() {
            assert_eq!(fact.outputs, vec![fact.store_path.clone()]);
        } else {
            assert!(!fact.outputs.is_empty());
        }
    }
    let envelope = IdentityFactsEnvelope {
        schema_version: IDENTITY_FACTS_SCHEMA_VERSION,
        context: IDENTITY_FACTS_CONTEXT.into(),
        challenge: challenge.clone(),
        manifest: manifest.clone(),
    };
    let receipt = IdentityFactsReceipt {
        signature: sign(&envelope, token.as_bytes()).expect("the broker signs the envelope"),
        envelope,
    };

    // Actual Core client verification, including the context-derived MAC.
    let client = RuntimeBrokerClient::new(
        "http://127.0.0.1:9",
        token.clone(),
        ResourceOwner::Environment,
    )
    .expect("construct the Core broker client");
    let verified = client
        .verify_identity_facts(&receipt, &challenge)
        .expect("the Core verifier accepts the real broker receipt");

    // Actual SQLite consume and replay refusal.
    let db_path = std::env::temp_dir().join(format!("factory-idf-selftest-{}.sqlite", &id[..24]));
    let db = Database::open(&db_path)
        .await
        .expect("open the test database");
    db.issue_identity_facts_challenge(&challenge, Some("selftest"), "selftest-plan", 900)
        .await
        .expect("persist the PENDING challenge");
    let bound = BoundIdentityFacts::spec_cache_observation(verified.clone());
    let stored = db
        .consume_identity_facts(&challenge, &bound)
        .await
        .expect("consume the real receipt");
    assert_eq!(stored.observed_root, root);
    assert_eq!(stored.fact_count, manifest.fact_count as u32);
    assert!(
        db.consume_identity_facts(&challenge, &bound).await.is_err(),
        "a replayed nonce must be refused"
    );
    let db = std::sync::Arc::new(db);
    let mut raced = challenge.clone();
    raced.nonce = format!("selftest-nonce-race-{}", &id[..24]);
    raced.operation_id = format!("selftest-operation-race-{}", &id[..24]);
    db.issue_identity_facts_challenge(&raced, Some("selftest"), "selftest-plan", 900)
        .await
        .expect("persist the concurrent challenge");
    let raced_envelope = IdentityFactsEnvelope {
        schema_version: IDENTITY_FACTS_SCHEMA_VERSION,
        context: IDENTITY_FACTS_CONTEXT.into(),
        challenge: raced.clone(),
        manifest: manifest.clone(),
    };
    let raced_receipt = IdentityFactsReceipt {
        signature: sign(&raced_envelope, token.as_bytes())
            .expect("the broker signs the raced envelope"),
        envelope: raced_envelope,
    };
    let raced_verified = client
        .verify_identity_facts(&raced_receipt, &raced)
        .expect("the raced receipt verifies");
    let raced_bound = BoundIdentityFacts::spec_cache_observation(raced_verified);
    let first = {
        let db = std::sync::Arc::clone(&db);
        let raced = raced.clone();
        let raced_bound = raced_bound.clone();
        tokio::spawn(async move { db.consume_identity_facts(&raced, &raced_bound).await })
    };
    let second = {
        let db = std::sync::Arc::clone(&db);
        let raced = raced.clone();
        let raced_bound = raced_bound.clone();
        tokio::spawn(async move { db.consume_identity_facts(&raced, &raced_bound).await })
    };
    let results = [first.await.unwrap(), second.await.unwrap()];
    assert_eq!(
        results.iter().filter(|result| result.is_ok()).count(),
        1,
        "exactly one concurrent consume must win"
    );
    assert_eq!(
        results.iter().filter(|result| result.is_err()).count(),
        1,
        "the concurrent loser must be refused"
    );
    db.close().await;
    let _ = std::fs::remove_file(&db_path);

    println!(
        "{}",
        serde_json::json!({
            "status": "PASS",
            "image": image,
            "observedRoot": root,
            "factCount": manifest.fact_count,
            "closureManifestSha256": manifest.closure_manifest_sha256,
            "signature": verified.receipt().signature,
            "planSource": "PROJECT_COMMITTED",
            "packages": [],
            "replay": "REFUSED",
            "coreVerifier": "ACCEPTED",
        })
    );
}
