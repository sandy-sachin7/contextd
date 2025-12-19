# CONTEXTD — Semantic Filesystem Context Daemon

## -1. Mandatory Start Step (Non-Negotiable)

Before **any** implementation, analysis, or design work:

```bash
git clone https://github.com/sandy-sachin7/contextd.git
cd contextd
git checkout dev
```

* All work **must** start from this repository.
* No code, files, or experiments outside this repo.
* All branches, commits, and merges operate against this origin.
* If the repo state and local state diverge, **pull first, then reason**.

This constraint is absolute.

---

> **One-line description**: A local-first, privacy-preserving OS-level context daemon that turns your files, activity, and knowledge into queryable semantic context — without uploading anything to the cloud.

---

## 0. Foundational Premise (Non‑Negotiable)

* **Local-first**: Everything runs on the user’s machine.
* **Free forever**: No SaaS, no paid tiers, no telemetry.
* **Embarrassingly useful before impressive**.
* **Invisible infrastructure**: Users shouldn’t think about it after install.
* **Opinionated minimalism**: Fewer features, harder guarantees.

This project exists because operating systems treat files as dumb bytes and force humans to organize meaning manually.

---

## 1. System Role

> **System Role**: You are a hybrid of **Linus Torvalds (systems brutality, correctness over comfort)** and a **Principal OpenAI Architect (LLM-aware abstractions, agent interoperability)**.

You:

* Optimize for **correctness, simplicity, and inevitability**.
* Reject UI-first thinking. CLI and daemon come first.
* Prefer boring, battle-tested tech over novelty.
* Write code that can survive 10 years of neglect.

You do NOT:

* Over-abstract prematurely.
* Introduce cloud dependencies.
* Chase trends.

---

## 2. Problem Statement

Modern operating systems:

* Index files by **path**, not **meaning**.
* Force apps to hoard user data to be useful.
* Break privacy by default in the AI era.

Current AI tooling:

* Requires uploading private data to third parties.
* Rebuilds context per app, per model, per session.

**This is a structural failure.**

---

## 3. The Idea (High-Level Architecture)

### Core Concept

Introduce a **Context Daemon** that:

* Runs locally in the background.
* Semantically indexes user data into embeddings.
* Exposes a **standard localhost API** for querying context.

Apps ask the OS:

> “What does the user already know, see, or work on that is relevant to this task?”

The OS answers — privately.

---

## 4. Name

### Project Name

**CONTEXTD**

* UNIX-style daemon naming
* Boring, inevitable, infrastructural
* Reads as *“context daemon”*

Binary: `contextd`

---

## 5. Non-Goals (Explicitly Out of Scope)

* Sync across devices
* UI dashboards
* Cloud inference
* Social features
* Model training

If it smells like Notion, kill it.

---

## 6. Technology Choices (Hard Constraints)

### Language

* **Rust** (memory safety, daemon reliability)

### Embeddings

* Pluggable interface
* Default: local ONNX or GGUF embedding model

### Vector Store

* Embedded, local (e.g., SQLite + HNSW)

### IPC / API

* HTTP over localhost
* JSON

### Platforms (Phased)

* Phase 1–2: Linux
* Phase 3: macOS
* Phase 4: Windows

---

## 7. Core Components

### 7.1 Context Daemon (`contextd`)

Responsibilities:

* File watching
* Content extraction
* Embedding generation
* Storage
* Query handling

Runs as:

* User-level service

---

### 7.2 Indexers (Pluggable)

Initial indexers:

* Plain text files
* Markdown
* PDFs
* Source code (language-agnostic text)

Each indexer:

* Emits `(content, metadata, timestamp, source)`

---

### 7.3 Context Store

Stores:

* Embeddings
* Metadata
* Source references

Constraints:

* Append-only where possible
* Deterministic rebuilds

---

### 7.4 Query Engine

Accepts:

* Natural language queries
* Filters (time, source, file type)

Returns:

* Ranked context chunks
* Provenance metadata

---

## 8. API Specification (v0)

### POST `/query`

```json
{
  "query": "Draft a reply based on the PDF I read yesterday",
  "limit": 5,
  "filters": {
    "since": "24h",
    "source": ["pdf"]
  }
}
```

### Response

```json
{
  "results": [
    {
      "content": "...",
      "source": "file:///home/user/docs/report.pdf",
      "timestamp": "2025-01-01T10:00:00Z",
      "score": 0.89
    }
  ]
}
```

---

## 9. Phases

### Phase 0 — Repo & Discipline

* Create GitHub repository: `<GITHUB_REPO_URL>`
* Branches:

  * `main` (always stable)
  * `dev`
  * `feature/*`

**Commit Rules**:

* Small commits
* One concern per commit
* Conventional format:

  * `core:`
  * `daemon:`
  * `api:`
  * `indexer:`
  * `docs:`

---

### Phase 1 — Minimal Daemon

**Goal**: A daemon that runs and indexes text files.

Deliverables:

* `contextd` starts/stops
* Watches a directory
* Indexes `.txt` and `.md`
* Accepts `/query`

Acceptance Criteria:

* Cold start < 1s
* Index rebuild works
* Query returns deterministic results

Tests:

* Daemon lifecycle test
* File add/update/delete
* Query correctness

---

### Phase 2 — Semantic Context (Prototype Milestone)

**Goal**: Real semantic value.

Add:

* PDF indexing
* Local embedding model
* Time-based filtering

Acceptance Criteria:

* Query relevance beats keyword search
* No network calls
* CPU usage remains bounded

Verification:

* Manual queries
* Re-index stress test

Outcome:

* Working local semantic context engine

---

### Phase 3 — Extensibility

Add:

* Plugin system for indexers
* Config file
* Better metadata

---

### Phase 4 — Cross‑Platform

* macOS service
* Windows service

---

## 10. Edge Cases

* Large files (>100MB)
* Rapid file churn
* Binary masquerading as text
* Model failures
* Disk full

Daemon must degrade gracefully.

---

## 11. Verification Strategy

* Deterministic rebuilds
* Replay indexing
* Snapshot comparisons

If rebuild ≠ original → bug.

---

## 12. Definition of Done (v0)

* Installs via single binary
* Runs silently
* Provides useful context
* Zero cloud dependency
* README explains everything

---

## 13. Final Principle

> *If removed tomorrow, power users feel amputated.*

If not, keep cutting.

---

## 14. Systems Brutality Audit (Torvalds Constraints)

These are **hard constraints**, not suggestions. Violate them and the daemon dies in real-world usage.

### 14.1 Battery & CPU Discipline

**Risk**: Continuous embedding on file writes will drain battery and spike CPU.

**Mandatory Mitigation**:

* Implement **adaptive debouncing** for indexing.
* Queue file changes.
* Process embeddings only when:

  * System is idle **OR**
  * A rate-limit window expires.
* Hard cap CPU usage per minute.

If `contextd` causes sustained >5–7% CPU on a laptop during normal editing, it is considered a bug.

---

### 14.2 Garbage Control (.contextignore)

**Risk**: Indexing noise destroys semantic quality.

**Mandatory Rules**:

* Introduce `.contextignore` (enabled by default).
* Preload ignores for:

  * `.git/`
  * `node_modules/`
  * `target/`
  * `dist/`
  * `__pycache__/`
  * Logs, binaries, generated assets

User overrides are allowed, but defaults are strict.

---

### 14.3 IPC Transport Discipline

* v0: HTTP over localhost is acceptable.
* v1: **Unix Domain Sockets (UDS)** on Linux/macOS are mandatory.

Rationale:

* File-permission–based security
* Lower latency
* No TCP port exposure

---

## 15. AI Architecture Audit (LLM-Aware Constraints)

### 15.1 Semantic Chunking (Non-Negotiable)

Embedding whole files is forbidden.

Chunking rules:

* **Code**: split by function / class / top-level block.
* **Markdown / Text**: split by headers and paragraphs.
* **PDFs**: split by detected sections/pages.

Each chunk must retain:

* Parent file reference
* Local context window (before/after)

If chunking fails, retrieval quality collapses.

---

### 15.2 Default Embedding Model

Ship with a **single default model**:

* `all-MiniLM-L6-v2` (quantized, CPU-friendly)

Constraints:

* ≤ 30MB
* No GPU required
* English-first, acceptable multilingual fallback

Model selection UI or config **does not exist** in v0.

---

## 16. Execution Plan — Phase 0 (Plumbing First)

No features before skeleton integrity is proven.

### 16.1 Mandatory Stack

* Language: Rust (stable)
* Async runtime: `tokio`
* DB: `sqlite` + vector extension
* Embeddings: ONNX Runtime (via Rust bindings)

---

### 16.2 Canonical Directory Structure

```
contextd/
├── Cargo.toml
├── src/
│   ├── main.rs          # CLI + entry point
│   ├── daemon.rs       # Process lifecycle
│   ├── config.rs       # Configuration structs
│   ├── api/
│   │   └── mod.rs      # Local API server
│   ├── indexer/
│   │   ├── mod.rs
│   │   ├── watcher.rs  # notify-rs integration
│   │   └── chunker.rs  # Semantic chunking logic
│   └── storage/
│       ├── mod.rs
│       └── db.rs       # SQLite + vector search
└── README.md
```

Deviation from this structure requires justification.

---

## 17. The First Irreversible Step (The "Git" Moment)

Before implementing logic:

* Design the **storage schema** completely.
* Schema must support:

  * Deterministic rebuilds
  * Provenance tracking
  * Time-based filtering

If the schema changes after indexing real data, the system has failed its architectural test.

---

## 18. Repository

**Canonical Repository**:
[https://github.com/sandy-sachin7/contextd.git](https://github.com/sandy-sachin7/contextd.git)

All development, branches, and history flow from this source.

---

## 19. Strategic Roadmap (v1)

### 19.1 Model Choice Trade-offs
*   **Current**: `all-MiniLM-L6-v2` (Fast, 384d, General text).
*   **Future Options**:
    *   `codebert-base`: Better for code understanding.
    *   `unixcoder`: Strong alternative for code.
*   **Action**: Evaluate trade-offs between speed/size and code-specific accuracy.

### 19.2 Chunking Strategy Improvements
*   **Markdown**: Preserve heading hierarchy in metadata for better context.
*   **Rust**: Capture doc comments with associated functions.
*   **PDF**: Improve page-level chunking to respect paragraph boundaries.

### 19.3 Search Quality Enhancements
*   **Re-ranking**: Implement cross-encoder for top-K results.
*   **Hybrid Search**: Combine semantic search with BM25 (keyword) for better precision.
*   **Metadata Filtering**: Enable queries like "only .rs files modified this week".

### 19.4 Production Readiness
*   **Incremental Updates**: Avoid re-embedding unchanged files.
*   **Deduplication**: Strategy to handle similar chunks.
*   **Resource Management**: Memory usage limits for large codebases.
*   **Observability**: Progress tracking for initial scans.
*   **Caching**: Query caching layer.

### 19.5 Developer Experience
*   **Installation**: Auto-download models or package them.
*   **CLI**: `contextd query "auth system"` for quick access.
*   **UI**: Simple Web UI for exploration.
*   **Docs**: Better configuration examples.

### 19.6 Prioritized Next Steps
1.  **Better Chunking**: Directly impacts search quality.
2.  **Hybrid Search**: Semantic + Keyword.
3.  **CLI Tool**: Lower barrier to entry.
4.  **Benchmarks**: Show speed/accuracy vs alternatives.

