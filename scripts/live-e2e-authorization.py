#!/usr/bin/env python3
"""Fail-closed authorization preflight for paid Factory E2E entrypoints.

This validates an explicit, invocation-scoped operator authorization envelope.
It is not a substitute for Core authorization or external provider evidence.
"""
from __future__ import annotations

import argparse
import os
import re
import sys


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--campaign", required=True)
    parser.add_argument("--project", required=True)
    parser.add_argument("--max-sessions", required=True, type=int)
    parser.add_argument("--route", action="append", required=True)
    parser.add_argument("--requested-route", action="append", required=True)
    args = parser.parse_args()

    def reject(message: str) -> int:
        print(f"[BLOCKED] live authorization: {message}", file=sys.stderr)
        return 2

    if os.environ.get("FACTORY_LIVE_MODE") != "1":
        return reject("FACTORY_LIVE_MODE=1 is required")
    if os.environ.get("FACTORY_LIVE_AUTHORIZATION") != "APPROVED":
        return reject("explicit FACTORY_LIVE_AUTHORIZATION=APPROVED is required")

    campaign = os.environ.get("FACTORY_LIVE_AUTH_CAMPAIGN", "")
    project = os.environ.get("FACTORY_LIVE_AUTH_PROJECT", "")
    budget_text = os.environ.get("FACTORY_LIVE_AUTH_SESSION_BUDGET", "")
    routes_text = os.environ.get("FACTORY_LIVE_AUTH_ROUTES", "")

    if campaign != args.campaign:
        return reject(f"campaign must be exactly {args.campaign}")
    if project != args.project:
        return reject(f"project must be exactly {args.project}")
    if not re.fullmatch(r"[1-9][0-9]*", budget_text):
        return reject("session budget must be an explicit positive integer")
    budget = int(budget_text)
    if budget > args.max_sessions:
        return reject(f"session budget {budget} exceeds entrypoint ceiling {args.max_sessions}")
    if budget < len(args.requested_route):
        return reject("session budget is below the number of requested paid sessions")

    allowed = args.route
    requested = args.requested_route
    if len(set(allowed)) != len(allowed) or len(set(requested)) != len(requested):
        return reject("duplicate routes are not allowed")
    if any(route not in allowed for route in requested):
        return reject("requested provider/model route is not allowed by this entrypoint")
    authorized_routes = routes_text.split(",") if routes_text else []
    if any(not route or route.strip() != route for route in authorized_routes):
        return reject("authorized routes must be a comma-separated list without whitespace")
    if len(set(authorized_routes)) != len(authorized_routes):
        return reject("duplicate authorized routes are not allowed")
    if set(authorized_routes) != set(requested):
        return reject("authorized provider/model routes do not exactly match this invocation")

    print(
        "[AUTHORIZED] explicit bounded live E2E invocation "
        f"campaign={campaign} project={project} budget={budget} routes={','.join(requested)}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
