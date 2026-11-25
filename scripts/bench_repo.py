#!/usr/bin/env python3
"""
Reproducible end-to-end codebase benchmark for `contextd`.

Indexes a real repository (default: the `contextd` repo itself) into an isolated temporary
SQLite database using the local ONNX embedding model, then measures:
  - Cold initial indexing wall-clock time, files/chunks indexed, DB size on disk, daemon RSS
  - Warm HTTP hybrid search (`POST /query`) p50 / p95 latency across 15 realistic code queries
  - Cold CLI (`contextd query`) latency
  - Concrete top-3 retrieval results on semantic queries (where literal keyword grep struggles)

Usage:
    python3 scripts/bench_repo.py [--target /path/to/repo]
"""
from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import socket
import statistics
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_BIN = REPO_ROOT / "target" / "release" / "contextd"


def find_contextd_bin() -> str:
    env_bin = os.environ.get("CONTEXTD_BIN")
    if env_bin:
        return env_bin
    if DEFAULT_BIN.exists():
        return str(DEFAULT_BIN)
    found = shutil.which("contextd")
    if found:
        return found
    raise FileNotFoundError(
        f"contextd binary not found at {DEFAULT_BIN} or in PATH. Run `cargo build --release` first."
    )


def find_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def find_model_dir(model_type: str = "all-minilm-l6-v2") -> Path:
    candidates = [
        Path.home() / ".local" / "share" / "contextd" / "models",
        Path.home() / "Library" / "Application Support" / "contextd" / "models",
    ]
    for c in candidates:
        if (c / model_type).exists():
            return c
    raise FileNotFoundError(f"No cached {model_type} model found. Run `contextd setup` first.")


def get_rss_mb(pid: int) -> float | None:
    status_path = Path(f"/proc/{pid}/status")
    if not status_path.exists():
        return None
    for line in status_path.read_text().splitlines():
        if line.startswith("VmHWM:") or line.startswith("VmRSS:"):
            if line.startswith("VmHWM:"):
                kb = int(line.split()[1])
                return round(kb / 1024.0, 1)
    for line in status_path.read_text().splitlines():
        if line.startswith("VmRSS:"):
            kb = int(line.split()[1])
            return round(kb / 1024.0, 1)
    return None


def http_json(url: str, payload: dict | None = None) -> dict:
    data = json.dumps(payload).encode("utf-8") if payload is not None else None
    headers = {"Content-Type": "application/json"} if data is not None else {}
    req = urllib.request.Request(url, data=data, headers=headers)
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read().decode("utf-8"))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", type=Path, default=REPO_ROOT)
    parser.add_argument("--model", default="all-minilm-l6-v2")
    args = parser.parse_args()

    target_dir = args.target.resolve()
    bin_path = find_contextd_bin()
    model_dir = find_model_dir(args.model)
    version = subprocess.check_output([bin_path, "--version"], text=True).strip()
    bin_mb = round(Path(bin_path).stat().st_size / (1024 * 1024), 1)
    port = find_free_port()

    print(f"Binary: {version} ({bin_mb} MB) | Target: {target_dir}")

    with tempfile.TemporaryDirectory(prefix="contextd-bench-") as tmp:
        tmp_path = Path(tmp)
        db_path = tmp_path / "bench.db"
        cfg_path = tmp_path / "contextd.toml"
        cfg_path.write_text(
            f"""[server]
host = "127.0.0.1"
port = {port}

[storage]
db_path = {json.dumps(str(db_path))}
model_path = {json.dumps(str(model_dir))}
model_type = {json.dumps(args.model)}

[watch]
paths = [{json.dumps(str(target_dir))}]
debounce_ms = 500
max_file_size = 2097152

[chunking]
max_chunk_size = 1500
overlap = 150
""",
            encoding="utf-8",
        )

        t_start = time.perf_counter()
        proc = subprocess.Popen(
            [bin_path, "--config", str(cfg_path), "--trust-config", "daemon"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )

        scan_done = False
        scan_done_at: float | None = None
        stdout_lines: list[str] = []
        stderr_lines: list[str] = []

        import threading

        def _drain_stdout() -> None:
            nonlocal scan_done, scan_done_at
            assert proc.stdout is not None
            for line in proc.stdout:
                stdout_lines.append(line)
                if "Daemon main loop starting" in line:
                    scan_done = True
                    scan_done_at = time.perf_counter()

        def _drain_stderr() -> None:
            assert proc.stderr is not None
            for line in proc.stderr:
                stderr_lines.append(line)

        threading.Thread(target=_drain_stdout, daemon=True).start()
        threading.Thread(target=_drain_stderr, daemon=True).start()

        base_url = f"http://127.0.0.1:{port}"
        try:
            # Wait until HTTP server is up
            ready = False
            for _ in range(300):
                if proc.poll() is not None:
                    raise RuntimeError(f"contextd exited early: {''.join(stderr_lines)}")
                try:
                    http_json(f"{base_url}/health")
                    ready = True
                    break
                except Exception:
                    time.sleep(0.05)
            if not ready:
                raise RuntimeError("Timed out waiting for contextd /health")

            # Wait for daemon to log "Daemon main loop starting" (initial scan complete)
            while not scan_done and (time.perf_counter() - t_start < 180):
                if proc.poll() is not None:
                    raise RuntimeError(f"contextd exited during scan: {''.join(stderr_lines)}")
                time.sleep(0.05)

            status_data = http_json(f"{base_url}/status")
            index_wall_s = round((scan_done_at or time.perf_counter()) - t_start, 2)
            peak_rss_mb = get_rss_mb(proc.pid)

            # Calculate DB + WAL disk size
            db_bytes = sum(
                p.stat().st_size
                for p in tmp_path.glob("bench.db*")
                if p.is_file()
            )

            queries = [
                "DNS rebinding host header validation",
                "symlink traversal protection when reading files",
                "reciprocal rank fusion hybrid vector and full text search",
                "tree sitter AST splitting for Rust and Python",
                "SHA-256 verification of downloaded ONNX embedding model",
                "MCP JSON-RPC stdio server tool dispatch",
                "auto configure Claude Code Cursor and Codex config files",
                "untrusted workspace config plugin isolation",
                "SQLite WAL reader connection pool",
                "incremental file hashing with blake3 to skip unchanged files",
                "bearer token constant time comparison",
                "PDF text extraction chunking",
                "file watcher debouncing and per path coalescing",
                "recency decay and access frequency scoring",
                "graceful shutdown on stdin EOF",
            ]

            # Warmup 2 queries
            for q in queries[:2]:
                http_json(f"{base_url}/query", {"query": q, "limit": 5})

            http_latencies_ms = []
            sample_hits = []
            for q in queries:
                qt0 = time.perf_counter()
                resp = http_json(f"{base_url}/query", {"query": q, "limit": 5})
                elapsed = (time.perf_counter() - qt0) * 1000.0
                http_latencies_ms.append(elapsed)
                results_list = resp if isinstance(resp, list) else resp.get("results", [])
                top_files = [
                    os.path.relpath(r.get("file_path", r.get("path", "")), target_dir)
                    for r in results_list[:3]
                ]
                sample_hits.append({"query": q, "ms": round(elapsed, 2), "top3": top_files})

            peak_rss_after_queries_mb = get_rss_mb(proc.pid)
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()

        # Measure cold CLI `contextd query` (loads ONNX model + queries SQLite)
        cli_times_ms = []
        for q in queries[:5]:
            t0 = time.perf_counter()
            subprocess.run(
                [bin_path, "--config", str(cfg_path), "--trust-config", "query", q, "--limit", "5"],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=True,
            )
            cli_times_ms.append((time.perf_counter() - t0) * 1000.0)

        http_sorted = sorted(http_latencies_ms)
        p95_idx = min(len(http_sorted) - 1, int(round(0.95 * (len(http_sorted) - 1))))

        out = {
            "version": version,
            "binary_mb": bin_mb,
            "host": f"{platform.system()} {platform.machine()} ({os.cpu_count()} logical CPUs)",
            "target_repo": target_dir.name,
            "status": status_data,
            "initial_index_wall_s": index_wall_s,
            "db_size_mb": round(db_bytes / (1024 * 1024), 2),
            "daemon_peak_rss_mb": peak_rss_after_queries_mb or peak_rss_mb,
            "http_query_p50_ms": round(statistics.median(http_latencies_ms), 2),
            "http_query_p95_ms": round(http_sorted[p95_idx], 2),
            "http_query_min_ms": round(min(http_latencies_ms), 2),
            "cli_cold_query_median_ms": round(statistics.median(cli_times_ms), 1),
            "sample_hits": sample_hits[:6],
        }
        print("\n=== JSON RESULTS ===")
        print(json.dumps(out, indent=2))


if __name__ == "__main__":
    main()
