#!/usr/bin/env python3
"""Deny-by-default HTTP CONNECT proxy for builder fetch evidence.

This is a development/CI test double for the runtime broker's credential-free
egress proxy, not the product broker. It accepts only an explicit host:port
allowlist, logs bounded request metadata, and refuses every other destination.
Nix/npm/Git clients receive it through HTTP(S)_PROXY.

Usage:
  scripts/environment-builder-egress-proxy.py \
    --listen 0.0.0.0 --port 18080 --allow cache.nixos.org:443,github.com:443 \
    --log /tmp/egress.log
"""

from __future__ import annotations

import argparse
import asyncio
import sys
from pathlib import Path

MAX_HEADER = 16 * 1024
MAX_LOG = 256 * 1024


class Proxy:
    def __init__(self, allow: set[str], log_path: Path) -> None:
        self.allow = allow
        self.log_path = log_path
        self.log_path.parent.mkdir(parents=True, exist_ok=True)
        self._log = self.log_path.open("a", encoding="utf-8")
        self._written = 0

    def record(self, line: str) -> None:
        if self._written >= MAX_LOG:
            return
        data = line[:2_000] + "\n"
        self._log.write(data)
        self._written += len(data.encode("utf-8", errors="replace"))
        self._log.flush()

    def allowed(self, host: str, port: int) -> bool:
        key = f"{host.lower()}:{port}"
        return key in self.allow

    async def handle(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        peer = writer.get_extra_info("peername")
        try:
            header = await asyncio.wait_for(reader.readuntil(b"\r\n\r\n"), timeout=15)
        except (asyncio.TimeoutError, asyncio.IncompleteReadError, asyncio.LimitOverrunError):
            writer.close()
            return
        if len(header) > MAX_HEADER:
            writer.close()
            return
        text = header.decode("latin-1", errors="replace")
        first = text.split("\r\n", 1)[0]
        parts = first.split()
        if len(parts) < 3:
            self.record(f"DENY malformed peer={peer} line={first[:200]}")
            writer.close()
            return
        method, target, _version = parts[0].upper(), parts[1], parts[2]
        host = ""
        port = 0
        if method == "CONNECT":
            host, _, port_text = target.partition(":")
            port = int(port_text) if port_text.isdigit() else 443
        else:
            from urllib.parse import urlsplit

            parsed = urlsplit(target)
            host = parsed.hostname or ""
            port = parsed.port or (443 if parsed.scheme == "https" else 80)
        if not self.allowed(host, port):
            self.record(f"DENY {method} {host}:{port} peer={peer}")
            writer.write(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
            await writer.drain()
            writer.close()
            return
        self.record(f"ALLOW {method} {host}:{port} peer={peer}")
        try:
            remote_reader, remote_writer = await asyncio.wait_for(
                asyncio.open_connection(host, port), timeout=20
            )
        except OSError:
            self.record(f"FAIL {method} {host}:{port}")
            writer.write(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
            await writer.drain()
            writer.close()
            return
        if method == "CONNECT":
            writer.write(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            await writer.drain()
            await self._pipe(reader, writer, remote_reader, remote_writer)
        else:
            remote_writer.write(header)
            await remote_writer.drain()
            await self._pipe(reader, writer, remote_reader, remote_writer)

    async def _pipe(self, reader, writer, remote_reader, remote_writer) -> None:
        async def pump(source, destination):
            try:
                while True:
                    data = await source.read(65_536)
                    if not data:
                        break
                    destination.write(data)
                    await destination.drain()
            except (ConnectionError, asyncio.CancelledError):
                pass
            finally:
                try:
                    destination.close()
                except Exception:
                    pass

        await asyncio.gather(
            pump(reader, remote_writer),
            pump(remote_reader, writer),
            return_exceptions=True,
        )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--listen", default="0.0.0.0")
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--allow", required=True)
    parser.add_argument("--log", default="/tmp/environment-builder-egress.log")
    args = parser.parse_args()
    allow = {entry.strip().lower() for entry in args.allow.split(",") if entry.strip()}
    if not allow:
        raise SystemExit("proxy needs a non-empty allowlist")
    proxy = Proxy(allow, Path(args.log))

    async def run() -> None:
        server = await asyncio.start_server(proxy.handle, args.listen, args.port)
        sockets = server.sockets or []
        port = sockets[0].getsockname()[1] if sockets else args.port
        print(port, flush=True)
        async with server:
            await server.serve_forever()

    try:
        asyncio.run(run())
    except KeyboardInterrupt:
        return 0
    return 0


if __name__ == "__main__":
    sys.exit(main())
