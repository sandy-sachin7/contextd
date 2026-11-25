#!/usr/bin/env python3
"""
Reproducible retrieval quality evaluation harness for `contextd`.

Compares five retrieval configurations on the exact same indexed corpus:
  1. ripgrep (`rg`) lexical baseline (files ranked by matching query terms)
  2. SQLite FTS5 only (`mode="fts"`)
  3. Vector KNN only (`mode="vector"`, recency=0, frequency=0)
  4. Hybrid RRF without recency/frequency (`mode="hybrid"`, recency=0, frequency=0)
  5. Hybrid RRF with default recency & frequency (`mode="hybrid"`)

Reports Recall@1, Recall@5, Recall@10, MRR, and nDCG@10 (overall and per query category).

Usage:
    cargo build --release --locked
    python3 scripts/eval_retrieval.py [--model all-minilm-l6-v2] [--out eval/results.json]
"""
from __future__ import annotations

import argparse
import json
import math
import os
import re
import shutil
import socket
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
from collections import Counter
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_BIN = REPO_ROOT / "target" / "release" / "contextd"
DEFAULT_QUERIES = REPO_ROOT / "eval" / "contextd_queries.json"

STOPWORDS = {
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "in",
    "into", "is", "it", "of", "on", "or", "that", "the", "this", "to", "via",
    "when", "with", "one",
}


def find_contextd_bin() -> str:
    env_bin = os.environ.get("CONTEXTD_BIN")
    if env_bin:
        return env_bin
    if DEFAULT_BIN.exists():
        return str(DEFAULT_BIN)
    found = shutil.which("contextd")
    if found:
        return found
    raise FileNotFoundError("contextd binary not found. Run `cargo build --release --locked` first.")


def find_model_dir(model_type: str) -> Path:
    candidates = [
        Path.home() / ".local" / "share" / "contextd" / "models",
        Path.home() / "Library" / "Application Support" / "contextd" / "models",
    ]
    for c in candidates:
        if (c / model_type / "model.onnx").exists():
            return c
    raise FileNotFoundError(
        f"Cached model '{model_type}' not found. Run `contextd setup` with model_type='{model_type}' first."
    )


def find_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def http_json(url: str, payload: dict | None = None) -> dict:
    data = json.dumps(payload).encode("utf-8") if payload is not None else None
    headers = {"Content-Type": "application/json"} if data is not None else {}
    req = urllib.request.Request(url, data=data, headers=headers)
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read().decode("utf-8"))


def ripgrep_rank_files(repo: Path, query: str, limit: int = 10) -> list[str]:
    tokens = [
        t for t in re.split(r"[^A-Za-z0-9_/]+", query)
        if len(t) >= 2 and t.lower() not in STOPWORDS
    ]
    if not tokens:
        return []

    file_distinct: Counter[str] = Counter()
    file_total: Counter[str] = Counter()

    for tok in set(tokens):
        proc = subprocess.run(
            ["rg", "-i", "-F", "--count-matches", "--glob", "!target/**", "--glob", "!eval/**", tok, str(repo)],
            capture_output=True,
            text=True,
            check=False,
        )
        if proc.returncode not in (0, 1):
            continue
        for line in proc.stdout.splitlines():
            if ":" not in line:
                continue
            fpath, cnt_str = line.rsplit(":", 1)
            try:
                cnt = int(cnt_str)
            except ValueError:
                continue
            rel = os.path.relpath(fpath, repo).replace("\\", "/")
            file_distinct[rel] += 1
            file_total[rel] += cnt

    ranked = sorted(
        file_distinct.keys(),
        key=lambda f: (file_distinct[f], file_total[f]),
        reverse=True,
    )
    return ranked[:limit]


def dedupe_ranked_files(results: list[dict], repo: Path) -> list[str]:
    seen: set[str] = set()
    ordered: list[str] = []
    for item in results:
        raw_path = item.get("file_path") or ""
        if not raw_path:
            continue
        rel = os.path.relpath(raw_path, repo).replace("\\", "/")
        if rel not in seen:
            seen.add(rel)
            ordered.append(rel)
    return ordered


def compute_metrics(ranked: list[str], expected: set[str]) -> dict[str, float]:
    r1 = 1.0 if any(f in expected for f in ranked[:1]) else 0.0
    r5 = 1.0 if any(f in expected for f in ranked[:5]) else 0.0
    r10 = 1.0 if any(f in expected for f in ranked[:10]) else 0.0

    mrr = 0.0
    for idx, f in enumerate(ranked[:10], start=1):
        if f in expected:
            mrr = 1.0 / idx
            break

    dcg = 0.0
    for idx, f in enumerate(ranked[:10], start=1):
        if f in expected:
            dcg += 1.0 / math.log2(idx + 1)
    ideal_hits = min(len(expected), 10)
    idcg = sum(1.0 / math.log2(idx + 1) for idx in range(1, ideal_hits + 1))
    ndcg10 = dcg / idcg if idcg > 0 else 0.0

    return {"recall@1": r1, "recall@5": r5, "recall@10": r10, "mrr": mrr, "ndcg@10": ndcg10}


def aggregate(rows: list[dict[str, float]]) -> dict[str, float]:
    n = len(rows)
    if n == 0:
        return {}
    keys = ("recall@1", "recall@5", "recall@10", "mrr", "ndcg@10")
    return {k: round(sum(r[k] for r in rows) / n, 4) for k in keys}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", default="all-minilm-l6-v2")
    parser.add_argument("--queries", type=Path, default=DEFAULT_QUERIES)
    parser.add_argument("--target", type=Path, default=REPO_ROOT)
    parser.add_argument("--out", type=Path, default=None)
    args = parser.parse_args()

    target_dir = args.target.resolve()
    bin_path = find_contextd_bin()
    model_dir = find_model_dir(args.model)
    queries = json.loads(args.queries.read_text(encoding="utf-8"))
    port = find_free_port()

    with tempfile.TemporaryDirectory(prefix="contextd-eval-") as tmp:
        tmp_path = Path(tmp)
        eval_repo = tmp_path / "repo"
        shutil.copytree(
            target_dir,
            eval_repo,
            ignore=shutil.ignore_patterns(".git", "target", "eval", "node_modules", "__pycache__"),
        )
        db_path = tmp_path / "eval.db"
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
paths = [{json.dumps(str(eval_repo))}]
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
        stderr_lines: list[str] = []

        def _drain_stdout() -> None:
            nonlocal scan_done, scan_done_at
            assert proc.stdout is not None
            for line in proc.stdout:
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

            while not scan_done and (time.perf_counter() - t_start < 300):
                if proc.poll() is not None:
                    raise RuntimeError(f"contextd exited during scan: {''.join(stderr_lines)}")
                time.sleep(0.05)

            index_s = round((scan_done_at or time.perf_counter()) - t_start, 2)
            status = http_json(f"{base_url}/status")

            modes = [
                ("ripgrep", None),
                ("fts_only", {"mode": "fts", "limit": 25}),
                ("vector_only", {"mode": "vector", "limit": 25, "recency_weight": 0.0, "frequency_weight": 0.0}),
                ("hybrid_rrf", {"mode": "hybrid", "limit": 25, "recency_weight": 0.0, "frequency_weight": 0.0}),
                ("hybrid_default", {"mode": "hybrid", "limit": 25}),
            ]

            summary: dict[str, dict] = {}
            for name, req_extra in modes:
                # Reset query_hits between modes so frequency signals never leak across evaluations.
                if db_path.exists():
                    conn = sqlite3.connect(str(db_path), timeout=10)
                    conn.execute("DELETE FROM query_hits")
                    conn.commit()
                    conn.close()
                all_scores = []
                by_cat: dict[str, list[dict[str, float]]] = {}
                for q in queries:
                    expected = set(q["expected_files"])
                    if name == "ripgrep":
                        ranked = ripgrep_rank_files(eval_repo, q["query"], limit=10)
                    else:
                        payload = {"query": q["query"], **req_extra}
                        resp = http_json(f"{base_url}/query", payload)
                        ranked = dedupe_ranked_files(resp.get("results", []), eval_repo)
                    m = compute_metrics(ranked, expected)
                    all_scores.append(m)
                    by_cat.setdefault(q["category"], []).append(m)

                summary[name] = {
                    "overall": aggregate(all_scores),
                    "by_category": {cat: aggregate(vals) for cat, vals in by_cat.items()},
                }
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()

    report = {
        "model": args.model,
        "query_count": len(queries),
        "indexed_files": status.get("indexed_files"),
        "total_chunks": status.get("total_chunks"),
        "index_wall_s": index_s,
        "chunks_per_sec": round((status.get("total_chunks") or 0) / max(index_s, 0.01), 1),
        "methods": summary,
    }

    print(f"\nModel: {args.model} | Indexed {report['indexed_files']} files / {report['total_chunks']} chunks in {index_s}s ({report['chunks_per_sec']} chunks/s)")
    print(f"{'Method':<16} {'R@1':>7} {'R@5':>7} {'R@10':>7} {'MRR':>7} {'nDCG@10':>8} | {'Sem MRR':>7} {'Exact MRR':>9} {'Mix MRR':>7}")
    print("-" * 88)
    for name, data in summary.items():
        ov = data["overall"]
        bc = data["by_category"]
        print(
            f"{name:<16} {ov['recall@1']:>7.3f} {ov['recall@5']:>7.3f} {ov['recall@10']:>7.3f} "
            f"{ov['mrr']:>7.3f} {ov['ndcg@10']:>8.3f} | "
            f"{bc.get('semantic', {}).get('mrr', 0):>7.3f} "
            f"{bc.get('exact', {}).get('mrr', 0):>9.3f} "
            f"{bc.get('mixed', {}).get('mrr', 0):>7.3f}"
        )

    if args.out is not None:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(f"\nSaved report to {args.out}")


if __name__ == "__main__":
    main()
