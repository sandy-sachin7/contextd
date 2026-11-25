# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 4.0.2+  | :white_check_mark: |
| 4.0.1   | :x: (Deprecated; use 4.0.2) |
| 3.x     | :x: (End of Life)  |
| < 3.0   | :x: (End of Life)  |

## Accepted Dependency Advisories

The following transitive, unmaintained advisories are accepted for 4.0.2. The repository owner approved this exception on 2026-11-25. Review it again when an upstream patch lands or at the next minor release.

| Advisory | Dependency path | Reason |
| -------- | --------------- | ------ |
| RUSTSEC-2025-0119 | `number_prefix` via `indicatif 0.17` | No upstream patch is available; exposure is limited to progress-bar rendering. |
| RUSTSEC-2024-0436 | `paste` via `tokenizers 0.21` | Crate is archived with no patch; exposure is build-time macro expansion. |
| RUSTSEC-2026-0192 | `ttf-parser` via `lopdf` through `pdf-extract 0.12` | No upstream patch is available; PDF parsing runs only on explicitly indexed PDFs. |

## Reporting a Vulnerability

We take security vulnerabilities in `contextd` seriously. **Please do not open a public GitHub issue for security reports.**

Report vulnerabilities privately through GitHub Security Advisories:

- **Private Advisory Form**: [https://github.com/sandy-sachin7/contextd/security/advisories/new](https://github.com/sandy-sachin7/contextd/security/advisories/new)
- **Fallback Contact**: `santhoshsachin7@gmail.com` (if you cannot access GitHub)

Please include:
- A clear description of the vulnerability and its impact
- Affected `contextd` version, OS, and configuration (`daemon`, `mcp`, `query`, `connect`, or library API)
- Minimal reproduction steps or proof-of-concept

We aim to acknowledge reports within **48 hours** and provide an initial assessment and remediation plan within **5 business days**.

## Threat Model & Trust Boundaries

`contextd` is designed to index local repositories — including **untrusted third-party repositories** — safely on developer machines and in containers.

| Boundary | Threat Mitigated | Enforcement Mechanism |
| -------- | ---------------- | --------------------- |
| **Workspace Config (`./contextd.toml`)** | Malicious repo hijacking bind address, DB path, or executing arbitrary plugin commands | Untrusted workspace configs may only set `[watch]` and `[chunking]`. `[server]` and `[storage]` come from user-global config/defaults. `[plugins]` requires explicit `--trust-workspace-plugins`, `--trust-config`, or user-global `security.allow_workspace_plugins = true`. |
| **External Plugins (`[plugins]`)** | Flag injection via filenames starting with `-`, hung processes, memory exhaustion | Arguments passed after `--` separator with canonical path; 30s timeout, 10 MiB stdout cap, `kill_on_drop`, UTF-8 validation. |
| **Filesystem Traversal (`safe_read`)** | Symlink escape / TOCTOU reads outside watch roots | `O_NOFOLLOW` directory-by-directory `openat` on Unix; `FILE_FLAG_OPEN_REPARSE_POINT` + final path verification on Windows. |
| **Sensitive & Binary Files** | Accidental indexing and leakage of credentials or binary blobs | Default exclusions for `.env*`, `*.pem`, `*.key`, `id_rsa`/`id_ed25519`, `.netrc`, `.npmrc`, `.pypirc`, `.git-credentials`, plus 8 KiB binary sniffing and `.gitignore`/`.contextignore`. |
| **HTTP API (`axum`)** | Browser DNS rebinding, unauthenticated network exposure, DoS | Loopback default (`127.0.0.1:3030`) with strict `Host` validation; non-loopback binds refuse to start without `auth_token` / `CONTEXTD_AUTH_TOKEN` (constant-time Bearer check); 64 KiB body cap, 30s timeout, 64 concurrent request limit. |
| **MCP stdio (`contextd mcp`)** | Oversized JSON-RPC frame OOM, stdout protocol corruption, data leakage in logs | 16 MiB frame cap; diagnostics written strictly to `stderr` without logging indexed source code or query results. |
| **Model & Binary Supply Chain** | Tampered ONNX weights or release binaries | Pinned HuggingFace commit revisions verified via SHA-256; release installers verify `SHA256SUMS.txt`. |

## Out of Scope

- **Physical or same-user OS compromise**: Any process running as the same OS user can already read the user's source files, user-global config (`~/.config/contextd/contextd.toml`), and local SQLite index directly.
- **Explicitly trusted plugins / configs**: Passing `--trust-config`, `--trust-workspace-plugins`, or configuring `[plugins]` in user-global config intentionally delegates execution to those commands.
- **Denial of service by a local user against their own CLI invocation**: Passing intentionally malicious local CLI arguments or enormous explicitly-configured watch trees on your own machine.
- **Upstream vulnerabilities in third-party dependencies** unless exploitable through `contextd`'s exposed surfaces (we track these via daily `cargo audit` CI and Dependabot).
