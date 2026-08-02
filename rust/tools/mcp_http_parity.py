#!/usr/bin/env python3
"""mcp_http_parity — HTTP-transport byte-parity referee for `rac mcp` (ADR-098).

The stdio referee (`mcp_parity.py`) proves the frame processor's payloads are
byte-identical between engines. The HTTP transport reuses that exact processor,
so this referee's job is the *transport envelope*: it drives two `--transport
http` servers (Python oracle + Rust) over real HTTP and checks

  1. response BODIES are byte-identical for every request (the ADR-098 parity
     surface: "payload-identical to stdio for identical corpus bytes"), and
  2. HTTP STATUS semantics match on the edge cases (406 / 400 / 405 / 202).

Not compared (declared non-parity, like the stdio port's stance on argparse
usage-wrapping): uvicorn's Date/Server header bytes and its Python-specific
error prose — the SDK's incidental framing, not RAC's contract. The one known
status divergence is GET: the SDK opens an idle SSE stream (200); the Rust
server offers none and returns 405 (spec-permitted) — reported, not failed.

Both servers are mandatory-audit-on (ADR-098): the harness stands up an
audit-enabled corpus so they start. Engine commands are shell-split; the
harness appends `--transport http --root <corpus> --port <p> --path /mcp`.

Usage:
  mcp_http_parity.py --engine-a "<cmd>" --engine-b "<cmd>" [--six] [--out DIR]
Exit 0 iff every body matches and every status matches (GET divergence aside).
"""

from __future__ import annotations

import argparse
import json
import os
import shlex
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
# Never route loopback through an ambient HTTP(S)_PROXY — it would hang/deny.
_OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))
PROTOCOL_VERSION = "2025-06-18"
CLIENT_INFO = {"name": "mcp-http-parity", "version": "0.0.1"}
KNOWN_ID = "RAC-KWMW45KXHZJP"  # ADR-098 itself, copied into the corpus


def build_corpus(out: Path) -> Path:
    corpus = out / "corpus"
    shutil.rmtree(corpus, ignore_errors=True)
    (corpus / ".decided").mkdir(parents=True)
    (corpus / ".decided" / "config.yaml").write_text("audit:\n  enabled: true\n", encoding="utf-8")
    for name in ("adr-098-shared-http-mcp-serving.md", "adr-084-read-access-audit-recorder.md",
                 "adr-032-guide-stateless-reads.md"):
        src = REPO_ROOT / "rac" / "decisions" / name
        if src.is_file():
            shutil.copy(src, corpus / name)
    return corpus


class HttpServer:
    def __init__(self, cmd: list[str], corpus: Path, port: int, xdg: Path):
        env = dict(os.environ)
        for name, sub in (("HOME", "home"), ("XDG_STATE_HOME", "state"),
                          ("XDG_CONFIG_HOME", "config"), ("XDG_CACHE_HOME", "cache")):
            d = xdg / sub
            d.mkdir(parents=True, exist_ok=True)
            env[name] = str(d)
        self.audit_path = xdg / "audit.jsonl"
        env["DECIDED_AUDIT_PATH"] = str(self.audit_path)
        env["DECIDED_NO_CACHE"] = "1"
        self.port = port
        self.url = f"http://127.0.0.1:{port}/mcp"
        self._err = open(xdg / "stderr.log", "wb")
        self.proc = subprocess.Popen(
            [*cmd, "--transport", "http", "--root", str(corpus),
             "--port", str(port), "--path", "/mcp"],
            stdout=self._err, stderr=self._err, cwd=str(REPO_ROOT), env=env,
        )

    def wait_ready(self, timeout: float = 20.0) -> None:
        deadline = time.time() + timeout
        while time.time() < deadline:
            if self.proc.poll() is not None:
                raise RuntimeError(f"server exited early ({self.proc.returncode}); "
                                   f"stderr: {Path(self._err.name).read_text()[-800:]}")
            try:
                self.post('{"jsonrpc":"2.0","id":0,"method":"ping"}')
                return
            except urllib.error.URLError:
                time.sleep(0.2)
        raise RuntimeError("server did not become ready")

    def post(self, body: str, *, accept: str | None = "application/json, text/event-stream",
             content_type: str | None = "application/json", method: str = "POST",
             principal: str | None = None) -> tuple[int, bytes]:
        headers = {}
        if content_type is not None:
            headers["Content-Type"] = content_type
        if accept is not None:
            headers["Accept"] = accept
        if principal is not None:
            headers["X-AsDecided-Principal"] = principal
        req = urllib.request.Request(self.url, data=body.encode("utf-8"),
                                     headers=headers, method=method)
        try:
            with _OPENER.open(req, timeout=10) as resp:
                return resp.status, resp.read()
        except urllib.error.HTTPError as e:
            return e.code, e.read()

    def audit_records(self) -> list[dict]:
        if not self.audit_path.exists():
            return []
        return [
            json.loads(line)
            for line in self.audit_path.read_text(encoding="utf-8").splitlines()
            if line.strip()
        ]

    def close(self) -> None:
        self.proc.terminate()
        try:
            self.proc.wait(timeout=8)
        except Exception:
            self.proc.kill()
        self._err.close()


# Non-deterministic fields excluded from audit-record parity (a side file, not a
# wire surface): wall-clock, per-server session tag, measured duration.
_AUDIT_VOLATILE = {"ts", "session", "duration_ms"}


def _norm_audit(rec: dict) -> dict:
    return {k: v for k, v in rec.items() if k not in _AUDIT_VOLATILE}


def rpc(id_: int, method: str, params: dict | None = None) -> str:
    obj: dict = {"jsonrpc": "2.0", "id": id_, "method": method}
    if params is not None:
        obj["params"] = params
    return json.dumps(obj, separators=(",", ":"))


def body_cases(six: bool) -> list[tuple[str, str]]:
    cases = [
        ("initialize", rpc(1, "initialize", {"protocolVersion": PROTOCOL_VERSION,
                                             "capabilities": {}, "clientInfo": CLIENT_INFO})),
        ("ping", rpc(2, "ping")),
        ("prompts/list", rpc(3, "prompts/list", {})),
        ("resources/list", rpc(4, "resources/list", {})),
        ("get_summary", rpc(5, "tools/call", {"name": "get_summary", "arguments": {}})),
        ("get_artifact", rpc(6, "tools/call", {"name": "get_artifact", "arguments": {"id": KNOWN_ID}})),
        ("get_artifact-missing", rpc(7, "tools/call", {"name": "get_artifact", "arguments": {"id": "RAC-NOPE00000000"}})),
        ("search", rpc(8, "tools/call", {"name": "search_artifacts", "arguments": {"query": "audit"}})),
        ("find_decisions", rpc(9, "tools/call", {"name": "find_decisions", "arguments": {"topic": "http"}})),
        ("get_related", rpc(10, "tools/call", {"name": "get_related", "arguments": {"id": KNOWN_ID, "depth": 2}})),
        ("bad-args", rpc(11, "tools/call", {"name": "get_artifact", "arguments": {}})),
        ("unknown-tool", rpc(12, "tools/call", {"name": "no_such_tool", "arguments": {}})),
        ("unknown-method", rpc(13, "no/such/method")),
    ]
    # tools/list and retrieve_grounding only match the 6-tool (retrieval) oracle;
    # the Rust server always serves the 6-tool superset (like stdio's list tag).
    if six:
        cases.insert(2, ("tools/list", rpc(20, "tools/list", {})))
        cases.append(("retrieve_grounding", rpc(21, "tools/call",
                     {"name": "retrieve_grounding", "arguments": {"task": "audit over http"}})))
    return cases


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--engine-a", required=True)
    ap.add_argument("--engine-b", required=True)
    ap.add_argument("--six", action="store_true", help="6-tool oracle: compare tools/list + retrieve_grounding")
    ap.add_argument("--out", default=str(REPO_ROOT / "rust" / "mcp-http-out"))
    args = ap.parse_args()

    # Absolute: the servers run with cwd=REPO_ROOT, so a relative --out (e.g. CI's
    # working-directory: rust) would make --root resolve against the wrong dir.
    out = Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    corpus = build_corpus(out)
    a = HttpServer(shlex.split(args.engine_a), corpus, 8991, out / "a")
    b = HttpServer(shlex.split(args.engine_b), corpus, 8992, out / "b")
    failures: list[str] = []
    try:
        a.wait_ready()
        b.wait_ready()
        # 1. Body parity.
        for name, body in body_cases(args.six):
            (sa, pa), (sb, pb) = a.post(body), b.post(body)
            if sa != sb:
                failures.append(f"body[{name}]: status {sa} != {sb}")
            if pa != pb:
                failures.append(f"body[{name}]: bytes differ ({len(pa)} vs {len(pb)})")
                print(f"  DIFFER {name}\n   A: {pa[:200]!r}\n   B: {pb[:200]!r}", file=sys.stderr)
            else:
                print(f"  MATCH  body[{name}] ({len(pa)} bytes, status {sa})")
        # 2. Status semantics on edge cases.
        req = rpc(30, "tools/list", {})
        checks = [
            ("no-accept -> 406", dict(accept=None), 406),
            ("wrong content-type -> 400", dict(content_type="text/plain"), 400),
            ("DELETE -> 405", dict(method="DELETE"), 405),
            ("notification -> 202", None, 202),
        ]
        for name, kw, want in checks:
            if name.startswith("notification"):
                sa, _ = a.post('{"jsonrpc":"2.0","method":"notifications/initialized"}')
                sb, _ = b.post('{"jsonrpc":"2.0","method":"notifications/initialized"}')
            else:
                sa, _ = a.post(req, **kw)  # type: ignore[arg-type]
                sb, _ = b.post(req, **kw)  # type: ignore[arg-type]
            ok = sa == sb == want
            print(f"  {'MATCH ' if ok else 'DIFFER'} status[{name}]: A={sa} B={sb}")
            if not ok:
                failures.append(f"status[{name}]: A={sa} B={sb} want {want}")
        # malformed body -> 400 on both (message prose is a declared gap)
        sa, _ = a.post("{not json", content_type="application/json")
        sb, _ = b.post("{not json", content_type="application/json")
        ok = sa == sb == 400
        print(f"  {'MATCH ' if ok else 'DIFFER'} status[malformed -> 400]: A={sa} B={sb}")
        if not ok:
            failures.append(f"status[malformed]: A={sa} B={sb}")

        # 3. Attribution: an X-AsDecided-Principal call is recorded as that principal,
        #    flagged attribution="asserted" (ADR-098), on both engines.
        pcall = rpc(50, "tools/call", {"name": "get_summary", "arguments": {}})
        a.post(pcall, principal="alice@example.com")
        b.post(pcall, principal="alice@example.com")

        # 4. Audit-log parity: same records in the same order, field-for-field
        #    minus the volatile trio. (ADR-084 per-call audit-line writing.)
        ra = [_norm_audit(r) for r in a.audit_records()]
        rb = [_norm_audit(r) for r in b.audit_records()]
        if not ra:
            failures.append("audit: engine A wrote no records")
        if ra == rb and ra:
            print(f"  MATCH  audit ({len(ra)} records)")
            last = ra[-1]
            if last.get("principal") == "alice@example.com" and last.get("attribution") == "asserted":
                print("  MATCH  audit[X-AsDecided-Principal asserted]")
            else:
                failures.append(f"audit[principal]: {last.get('principal')}/{last.get('attribution')}")
        else:
            failures.append(f"audit: {len(ra)} vs {len(rb)} records differ")
            for i, (x, y) in enumerate(zip(ra, rb)):
                if x != y:
                    print(f"   audit rec {i} A={x}", file=sys.stderr)
                    print(f"   audit rec {i} B={y}", file=sys.stderr)
                    break
    except Exception as exc:  # surface setup/crash into the result file too
        import traceback
        failures.append("crash: " + "".join(traceback.format_exception(exc))[-1500:])
    finally:
        a.close()
        b.close()

    # Write a durable result file (stdout capture is unreliable under some
    # background/timeout wrappers); the caller reads this.
    mode = "six-tool" if args.six else "primary"
    summary = f"mcp_http_parity ({mode}): {'PASS' if not failures else 'FAIL'}"
    lines = [summary] + [f"  - {f}" for f in failures]
    (out / "result.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n".join(lines), flush=True)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
