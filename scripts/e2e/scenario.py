#!/usr/bin/env python3
"""Stage-addressed runner for the deterministic Factory Compose scenario.

Examples:
    python3 scripts/e2e/scenario.py --list
    python3 scripts/e2e/scenario.py --doctor
    python3 scripts/e2e/scenario.py --from prepare --to gate --keep
    python3 scripts/e2e/scenario.py --from gate --to summary --run-dir /var/tmp/factory-e2e/<id>
    python3 scripts/e2e/scenario.py --snapshot --provider fixture --github fake

The runner never calls a real model, GitHub or the network beyond loopback. It
is development/test infrastructure, not a Factory runtime component.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import time
from pathlib import Path

from common import (
    Api,
    Compose,
    Config,
    ScenarioError,
    State,
    cleanup,
    collect_diagnostics,
    doctor,
    ensure_dir,
    free_port,
    log,
    now_utc,
    project_name,
    snapshot_volumes,
    write_manifest,
)
from compose_intake import (
    STAGES,
    Context,
    stage_description,
    stage_function,
    stage_order,
)


def _default_run_id() -> str:
    return f"factory-e2e-{int(time.time())}-{os.getpid()}"


def _env_flag(name: str) -> bool:
    return os.environ.get(name, "").strip().lower() in {"1", "true", "yes", "on"}


def _load_scenario(path: str | None) -> tuple[str, list[str]]:
    if not path:
        return "compose-intake", stage_order()
    scenario_path = Path(path)
    try:
        payload = json.loads(scenario_path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise ScenarioError(f"scenario file does not exist: {scenario_path}") from error
    except json.JSONDecodeError as error:
        raise ScenarioError(f"scenario file is not valid JSON: {error}") from error
    stages = payload.get("stages")
    if not isinstance(stages, list) or not stages:
        raise ScenarioError("scenario.stages must be a non-empty list")
    known = stage_order()
    unknown = [name for name in stages if name not in known]
    if unknown:
        raise ScenarioError(f"scenario contains unknown stages: {', '.join(unknown)}")
    positions = [known.index(name) for name in stages]
    if positions != sorted(positions):
        raise ScenarioError("scenario stages must follow the canonical order")
    if len(set(stages)) != len(stages):
        raise ScenarioError("scenario stages must not repeat")
    return str(payload.get("name", scenario_path.stem)), stages


def _parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Stage-addressed deterministic Factory Compose scenario runner.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="\n".join(f"  {name:12} {description}" for name, _, description in STAGES),
    )
    parser.add_argument("--list", action="store_true", help="list stages and exit")
    parser.add_argument("--doctor", action="store_true", help="validate prerequisites and exit")
    parser.add_argument("--scenario", help="path to a JSON scenario (optional)")
    parser.add_argument("--from", dest="from_stage", help="first stage to run")
    parser.add_argument("--to", dest="to_stage", help="last stage to run")
    parser.add_argument("--run-dir", help="resume/create a specific run directory")
    parser.add_argument("--artifacts", help="artifact directory (default: <repo>/target/qh/<run-id>)")
    parser.add_argument("--snapshot", action="store_true", help="export Compose volumes after the run")
    parser.add_argument("--restore", action="store_true", help="restore volumes before starting")
    parser.add_argument("--snapshot-dir", help="snapshot directory (default: <run-dir>/snapshots)")
    parser.add_argument("--keep", action="store_true", help="keep containers, volumes and run directory")
    parser.add_argument("--fresh", action="store_true", help="discard prior state/cookies/git/model in the run directory")
    parser.add_argument("--no-build", action="store_true", help="start Compose without rebuilding images")
    parser.add_argument("--provider", choices=["fixture", "cassette", "real"], default=os.environ.get("FACTORY_E2E_PROVIDER", "fixture"))
    parser.add_argument("--cassette", help="validated provider cassette for --provider cassette")
    parser.add_argument("--github", choices=["fake", "sandbox", "real"], default=os.environ.get("FACTORY_E2E_GITHUB", "fake"))
    parser.add_argument("--browser", choices=["none", "chromium"], default=os.environ.get("FACTORY_E2E_BROWSER", "none"))
    parser.add_argument("--project", default=os.environ.get("FACTORY_E2E_PROJECT"))
    parser.add_argument("--port", type=int, default=int(os.environ.get("FACTORY_PORT", "0")))
    parser.add_argument("--timeout", type=int, default=int(os.environ.get("FACTORY_E2E_TIMEOUT", "600")))
    return parser.parse_args(argv)


def _make_config(args: argparse.Namespace) -> Config:
    run_id = _default_run_id()
    root = Path(__file__).resolve().parents[2]
    run_dir = Path(args.run_dir).expanduser() if args.run_dir else Path("/var/tmp/factory-e2e") / run_id
    artifacts_dir = (
        Path(args.artifacts).expanduser()
        if args.artifacts
        else root / "target" / "qh" / run_id
    )
    project = project_name(args.project or f"software-factory-e2e-{run_id.rsplit('-', 2)[-2]}-{run_id.rsplit('-', 1)[-1]}")
    port = args.port if args.port else free_port()
    snapshot_dir = Path(args.snapshot_dir).expanduser() if args.snapshot_dir else run_dir / "snapshots"
    scenario_name, stages = _load_scenario(args.scenario)

    from_stage = args.from_stage or stages[0]
    to_stage = args.to_stage or stages[-1]
    if from_stage not in stages:
        raise ScenarioError(f"--from {from_stage!r} is not in the selected scenario: {', '.join(stages)}")
    if to_stage not in stages:
        raise ScenarioError(f"--to {to_stage!r} is not in the selected scenario: {', '.join(stages)}")
    if stages.index(from_stage) > stages.index(to_stage):
        raise ScenarioError("--from must not be after --to")

    cassette = None
    if args.provider == "cassette":
        if not args.cassette:
            raise ScenarioError("--provider cassette requires --cassette <path>")
        from cassette import CassetteError, load as load_cassette

        try:
            loaded = load_cassette(Path(args.cassette).expanduser())
        except CassetteError as error:
            raise ScenarioError(f"invalid cassette: {error}") from error
        cassette = Path(args.cassette).expanduser()
        if not loaded.get("turns"):
            raise ScenarioError("cassette has no turns")
    elif args.provider == "real":
        raise ScenarioError(
            "--provider real requires a separate owner authorization envelope and is not "
            "part of this deterministic harness; use fixture or cassette."
        )
    elif args.cassette:
        raise ScenarioError("--cassette requires --provider cassette")
    if args.github != "fake":
        raise ScenarioError(
            "--github sandbox/real is not implemented by this harness; use fake. "
            "Core-owned real delivery remains a separate, authorized lane."
        )

    return Config(
        root=root,
        run_id=run_id,
        run_dir=run_dir,
        artifacts_dir=artifacts_dir,
        project=project,
        port=port,
        provider=args.provider,
        github=args.github,
        cassette=cassette,
        browser=args.browser,
        from_stage=from_stage,
        to_stage=to_stage,
        keep=args.keep or _env_flag("FACTORY_E2E_KEEP"),
        snapshot=args.snapshot,
        restore=args.restore,
        snapshot_dir=snapshot_dir,
        no_build=args.no_build,
        timeout=args.timeout,
        scenario=scenario_name,
    )


def _stage_slice(config: Config, stages: list[str]) -> list[str]:
    start = stages.index(config.from_stage)
    end = stages.index(config.to_stage)
    return stages[start : end + 1]


def _fresh(config: Config) -> None:
    for path in (
        config.state_path,
        config.cookie_jar,
        config.run_dir / "tmp",
        config.git_dir,
        config.model_dir,
    ):
        if path.is_dir():
            shutil.rmtree(path, ignore_errors=True)
        else:
            path.unlink(missing_ok=True)


def _record_stage(state: State, name: str, started: float, status: str, error: str | None = None) -> None:
    state.append(
        "stage_results",
        {
            "name": name,
            "status": status,
            "startedAt": now_utc(),
            "durationMs": int((time.monotonic() - started) * 1000),
            "error": error,
        },
    )
    state.save()


def main(argv: list[str]) -> int:
    try:
        args = _parse_args(argv)
        if args.list:
            for name, _, description in STAGES:
                print(f"{name:12} {description}")
            return 0

        config = _make_config(args)
        compose = Compose(config)
        api = Api(config)

        if args.doctor:
            result = doctor(config, compose)
            print(json.dumps(result, indent=2, sort_keys=True))
            return 0 if result["ok"] else 1

        ensure_dir(config.run_dir)
        ensure_dir(config.artifacts_dir)
        ensure_dir(config.snapshot_dir)
        if args.fresh:
            _fresh(config)

        scenario_name, stages = _load_scenario(args.scenario)
        selected = _stage_slice(config, stages)
        state_path = config.state_path
        if config.from_stage != stages[0] and not state_path.exists():
            raise ScenarioError(
                f"cannot resume from {config.from_stage!r}: {state_path} does not exist. "
                "Run the earlier stages first or omit --from."
            )
        if config.from_stage == stages[0] and state_path.exists() and not args.fresh:
            raise ScenarioError(
                f"run directory already contains state: {state_path}. "
                "Use --fresh to discard it, or resume with --from <later-stage>."
            )
        state = State.load_or_create(state_path)
        state.set("run_id", config.run_id)
        state.set("scenario", scenario_name)
        state.set("selected_stages", selected)
        state.set("provider", config.provider)
        state.set("github", config.github)
        state.set("browser", config.browser)
        state.save()

        context = Context(config=config, state=state, compose=compose, api=api)
        started_at = now_utc()
        log(f"scenario={scenario_name} run={config.run_id} project={config.project} port={config.port}")
        log(f"stages: {' -> '.join(selected)}")
        log(f"artifacts: {config.artifacts_dir}")
        if not config.keep:
            log("containers and volumes will be removed on completion; use --keep to retain them")

        for name in selected:
            started = time.monotonic()
            log(f"== {name}: {stage_description(name)}")
            try:
                stage_function(name)(context)
            except Exception as error:  # noqa: BLE001 - one bounded failure surface
                message = str(error)
                _record_stage(state, name, started, "FAIL", message)
                try:
                    collect_diagnostics(config, compose, state, api)
                except Exception as diagnostic_error:  # noqa: BLE001
                    log(f"diagnostics collection failed: {diagnostic_error}")
                status = "FAIL"
                manifest = write_manifest(
                    config,
                    state,
                    status=status,
                    started_at=started_at,
                    error=f"{name}: {message}",
                    scenarios_ran=selected,
                )
                log(f"FAILED {name}: {message}")
                log(f"manifest: {config.artifacts_dir / 'manifest.json'}")
                cleanup(config, compose)
                print(json.dumps(manifest, indent=2, sort_keys=True))
                return 1
            _record_stage(state, name, started, "PASS")
            log(f"PASS {name} ({int((time.monotonic() - started) * 1000)}ms)")

        status = "PASS" if selected[-1] == stage_order()[-1] else "PARTIAL"
        snapshot_error: str | None = None
        if config.snapshot:
            try:
                index = snapshot_volumes(config, compose)
                state.set("snapshot", index)
                state.save()
                log(f"snapshot: {config.snapshot_dir}")
            except Exception as error:  # noqa: BLE001
                snapshot_error = f"snapshot: {error}"
                status = "FAIL"
                log(snapshot_error)
        manifest = write_manifest(
            config,
            state,
            status=status,
            started_at=started_at,
            error=snapshot_error,
            scenarios_ran=selected,
        )
        log(f"{status}: {config.artifacts_dir / 'manifest.json'}")
        cleanup(config, compose)
        print(json.dumps(manifest, indent=2, sort_keys=True))
        return 0 if status in {"PASS", "PARTIAL"} else 1
    except ScenarioError as error:
        log(f"error: {error}")
        return 2
    except KeyboardInterrupt:
        log("interrupted")
        return 130


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
