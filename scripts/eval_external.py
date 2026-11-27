#!/usr/bin/env python3
"""
Independent external multi-language retrieval evaluation runner for `contextd`.

Clones third-party open-source repositories pinned to immutable commit SHAs in
`eval/external_repos.json` (Rust, Python, TypeScript), runs `scripts/eval_retrieval.py`
against each repository, aggregates weighted MRR / nDCG@10 / Recall@K across all
external queries, and optionally enforces automated regression gates.

Usage:
    cargo build --release --locked
    python3 scripts/eval_external.py --model all-minilm-l6-v2-q8 --out-dir eval
"""
from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_MANIFEST = REPO_ROOT / "eval" / "external_repos.json"
EVAL_SCRIPT = REPO_ROOT / "scripts" / "eval_retrieval.py"


def checkout_pinned_repo(entry: dict, cache_root: Path) -> Path:
    repo_dir = cache_root / entry["name"]
    commit = entry["commit"]
    if (repo_dir / ".git").exists():
        head = subprocess.run(
            ["git", "-C", str(repo_dir), "rev-parse", "HEAD"],
            capture_output=True,
            text=True,
            check=False,
        ).stdout.strip()
        if head == commit:
            return repo_dir

    repo_dir.mkdir(parents=True, exist_ok=True)
    if not (repo_dir / ".git").exists():
        subprocess.run(["git", "init", str(repo_dir)], check=True, stdout=subprocess.DEVNULL)
        subprocess.run(
            ["git", "-C", str(repo_dir), "remote", "add", "origin", entry["url"]],
            check=True,
        )
    subprocess.run(
        ["git", "-C", str(repo_dir), "fetch", "--depth", "1", "origin", commit],
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(repo_dir), "checkout", "--force", commit],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    return repo_dir


def run_all(args: argparse.Namespace, cache_root: Path) -> int:
    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    per_repo: list[dict] = []
    methods_order = ["ripgrep", "fts_only", "vector_only", "hybrid_rrf", "hybrid_default"]

    for entry in manifest:
        name = entry["name"]
        print(f"\n=== External repo: {name} ({entry['language']}, {entry['commit'][:10]}) ===")
        repo_dir = checkout_pinned_repo(entry, cache_root)
        queries_path = (REPO_ROOT / entry["queries"]).resolve()

        with tempfile.NamedTemporaryFile(suffix=".json", delete=False) as tf:
            tmp_report = Path(tf.name)
        try:
            out_path = (
                args.out_dir / f"results-external-{name}-q8.json"
                if args.out_dir is not None
                else tmp_report
            )
            cmd = [
                sys.executable,
                str(EVAL_SCRIPT),
                "--model",
                args.model,
                "--target",
                str(repo_dir),
                "--queries",
                str(queries_path),
                "--out",
                str(out_path),
            ]
            if args.auto_setup:
                cmd.append("--auto-setup")
            subprocess.run(cmd, check=True)
            report = json.loads(out_path.read_text(encoding="utf-8"))
            report["repo"] = name
            report["language"] = entry["language"]
            report["url"] = entry["url"]
            report["commit"] = entry["commit"]
            report["license"] = entry["license"]
            if args.out_dir is not None:
                out_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
            per_repo.append(report)
        finally:
            tmp_report.unlink(missing_ok=True)

    total_queries = sum(r["query_count"] for r in per_repo)
    weighted: dict[str, dict] = {}
    for m in methods_order:
        ov_keys = ("recall@1", "recall@5", "recall@10", "mrr", "ndcg@10")
        ov = {
            k: round(
                sum(r["methods"][m]["overall"][k] * r["query_count"] for r in per_repo)
                / total_queries,
                4,
            )
            for k in ov_keys
        }
        sem_mrr = round(
            sum(
                r["methods"][m]["by_category"].get("semantic", {}).get("mrr", 0.0)
                * r["query_count"]
                for r in per_repo
            )
            / total_queries,
            4,
        )
        weighted[m] = {"overall": ov, "semantic_mrr": sem_mrr}

    print(f"\n=== Weighted External Summary ({total_queries} queries across {len(per_repo)} third-party repos) ===")
    print(f"{'Method':<16} {'R@1':>7} {'R@5':>7} {'R@10':>7} {'MRR':>7} {'nDCG@10':>8} | {'Sem MRR':>7}")
    print("-" * 70)
    for m in methods_order:
        ov = weighted[m]["overall"]
        print(
            f"{m:<16} {ov['recall@1']:>7.3f} {ov['recall@5']:>7.3f} {ov['recall@10']:>7.3f} "
            f"{ov['mrr']:>7.3f} {ov['ndcg@10']:>8.3f} | {weighted[m]['semantic_mrr']:>7.3f}"
        )

    summary_doc = {
        "model": args.model,
        "total_queries": total_queries,
        "repositories": [
            {
                "name": r["repo"],
                "language": r["language"],
                "url": r["url"],
                "commit": r["commit"],
                "license": r["license"],
                "query_count": r["query_count"],
                "indexed_files": r["indexed_files"],
                "total_chunks": r["total_chunks"],
                "index_wall_s": r["index_wall_s"],
                "ripgrep_mrr": r["methods"]["ripgrep"]["overall"]["mrr"],
                "ripgrep_ndcg10": r["methods"]["ripgrep"]["overall"]["ndcg@10"],
                "hybrid_default_mrr": r["methods"]["hybrid_default"]["overall"]["mrr"],
                "hybrid_default_ndcg10": r["methods"]["hybrid_default"]["overall"]["ndcg@10"],
            }
            for r in per_repo
        ],
        "weighted_methods": weighted,
    }
    if args.out_dir is not None:
        summary_path = args.out_dir / "results-external-summary-q8.json"
        summary_path.write_text(json.dumps(summary_doc, indent=2) + "\n", encoding="utf-8")
        print(f"\nSaved external summary to {summary_path}")

    failures: list[str] = []
    hyb = weighted["hybrid_default"]["overall"]
    rg = weighted["ripgrep"]["overall"]
    if args.min_mrr is not None and hyb["mrr"] < args.min_mrr:
        failures.append(f"external hybrid_default MRR {hyb['mrr']:.4f} < --min-mrr {args.min_mrr:.4f}")
    if args.min_ndcg is not None and hyb["ndcg@10"] < args.min_ndcg:
        failures.append(
            f"external hybrid_default nDCG@10 {hyb['ndcg@10']:.4f} < --min-ndcg {args.min_ndcg:.4f}"
        )
    if args.require_beat_ripgrep:
        if hyb["mrr"] <= rg["mrr"]:
            failures.append(
                f"external hybrid_default MRR {hyb['mrr']:.4f} did not beat ripgrep {rg['mrr']:.4f}"
            )
        if hyb["ndcg@10"] <= rg["ndcg@10"]:
            failures.append(
                f"external hybrid_default nDCG@10 {hyb['ndcg@10']:.4f} did not beat ripgrep {rg['ndcg@10']:.4f}"
            )
    if failures:
        print("\nEXTERNAL REGRESSION GATE FAILED:", file=sys.stderr)
        for msg in failures:
            print(f"  - {msg}", file=sys.stderr)
        return 1
    if args.min_mrr is not None or args.min_ndcg is not None or args.require_beat_ripgrep:
        print("\nEXTERNAL REGRESSION GATE PASSED")
    return 0


def main() -> None:
    parser = argparse.ArgumentParser(description="Run independent external multi-language retrieval eval")
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--model", default="all-minilm-l6-v2-q8")
    parser.add_argument("--cache-dir", type=Path, default=None)
    parser.add_argument("--out-dir", type=Path, default=None)
    parser.add_argument("--auto-setup", action="store_true")
    parser.add_argument("--min-mrr", type=float, default=None)
    parser.add_argument("--min-ndcg", type=float, default=None)
    parser.add_argument("--require-beat-ripgrep", action="store_true")
    args = parser.parse_args()

    if args.cache_dir is not None:
        args.cache_dir.mkdir(parents=True, exist_ok=True)
        sys.exit(run_all(args, args.cache_dir))
    else:
        with tempfile.TemporaryDirectory(prefix="contextd-ext-cache-") as tmp:
            sys.exit(run_all(args, Path(tmp)))


if __name__ == "__main__":
    main()
