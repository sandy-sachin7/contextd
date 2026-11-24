#!/usr/bin/env bash
set -euo pipefail

# contextd Playground
# One-line: curl -fsSL https://raw.githubusercontent.com/sandy-sachin7/contextd/main/scripts/playground.sh | bash
# Or locally: bash scripts/playground.sh

REPO="sandy-sachin7/contextd"
GITHUB="https://github.com/$REPO"

BOLD='\033[1m'
DIM='\033[2m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { printf "${GREEN}➜${NC} %s\n" "$*"; }
warn()  { printf "${YELLOW}⚠${NC} %s\n" "$*"; }
header(){ printf "\n${BOLD}${CYAN}━━━ %s ━━━${NC}\n" "$*"; }
cmd()   { printf "${DIM}\$ %s${NC}\n" "$*"; }

TMPDIR_WORK=""
DAEMON_PID=""

cleanup_all() {
  if [ -n "${DAEMON_PID:-}" ] && kill -0 "$DAEMON_PID" 2>/dev/null; then
    warn "Stopping daemon (PID: $DAEMON_PID)"
    kill "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
  fi
  if [ -n "${TMPDIR_WORK:-}" ] && [ -d "$TMPDIR_WORK" ]; then
    rm -rf "$TMPDIR_WORK"
  fi
}
trap cleanup_all EXIT INT TERM

# ── Detect platform (matches release.yml asset names) ────────
detect_binary_asset() {
  local os arch
  case "$(uname -s)" in
    Linux)  os="linux";;
    Darwin) os="macos";;
    *)      echo "Unsupported OS: $(uname -s)" >&2; exit 1;;
  esac
  case "$(uname -m)" in
    x86_64|amd64)  arch="x86_64";;
    aarch64|arm64) arch="aarch64";;
    *) echo "Unsupported arch: $(uname -m)" >&2; exit 1;;
  esac
  echo "contextd-${os}-${arch}"
}

verify_sha256() {
  local file="$1"
  local expected="$2"
  local actual=""
  if command -v sha256sum &>/dev/null; then
    actual="$(sha256sum "$file" | awk '{print $1}')"
  elif command -v shasum &>/dev/null; then
    actual="$(shasum -a 256 "$file" | awk '{print $1}')"
  else
    echo "Error: neither sha256sum nor shasum found to verify download integrity." >&2
    exit 1
  fi
  if [ "$actual" != "$expected" ]; then
    echo "Checksum verification failed! Expected $expected, got $actual" >&2
    exit 1
  fi
}

# ── Ensure contextd binary ───────────────────────────────────
ensure_binary() {
  if command -v contextd &>/dev/null; then
    info "contextd already installed at $(command -v contextd)"
    BINARY="contextd"
    return 0
  fi

  local asset
  asset="$(detect_binary_asset)"
  local url="${GITHUB}/releases/latest/download/${asset}"
  local sums_url="${GITHUB}/releases/latest/download/SHA256SUMS.txt"

  warn "contextd not found — downloading latest release (${asset})..."
  cmd "curl -fsSL $url"

  TMPDIR_WORK="$(mktemp -d)"
  curl -fsSL "$url" -o "$TMPDIR_WORK/$asset"
  curl -fsSL "$sums_url" -o "$TMPDIR_WORK/SHA256SUMS.txt"

  local expected_sha
  expected_sha="$(awk -v name="$asset" '$2 == name || $2 == ("*" name) { print $1; exit }' "$TMPDIR_WORK/SHA256SUMS.txt")"
  if [ -z "$expected_sha" ]; then
    echo "Failed to find SHA-256 entry for ${asset} in SHA256SUMS.txt" >&2
    exit 1
  fi
  verify_sha256 "$TMPDIR_WORK/$asset" "$expected_sha"
  chmod +x "$TMPDIR_WORK/$asset"

  local install_dir="$HOME/.local/bin"
  mkdir -p "$install_dir"
  mv "$TMPDIR_WORK/$asset" "$install_dir/contextd"
  BINARY="$install_dir/contextd"
  export PATH="$install_dir:$PATH"

  info "Installed verified contextd to $BINARY"
}

# ── Init model (auto-download on first run) ──────────────────
init_model() {
  local config_file="$1"
  header "Ensuring default model (all-MiniLM-L6-v2)"
  "$BINARY" --config "$config_file" setup
}

# ── Index workspace ──────────────────────────────────────────
index_workspace() {
  local workspace="${1:-.}"
  local config_file="$2"

  header "Indexing workspace"
  echo "  contextd will watch files in: ${workspace}"
  echo

  cat > "$config_file" <<-EOF
[server]
host = "127.0.0.1"
port = 3031

[storage]
db_path = "contextd-playground.db"
model_path = "models"
model_type = "all-minilm-l6-v2"

[watch]
paths = ["${workspace}"]
debounce_ms = 500
EOF

  init_model "$config_file"

  info "Starting daemon (indexing in background)..."
  cmd "$BINARY --config $config_file daemon &"
  "$BINARY" --config "$config_file" daemon &
  DAEMON_PID=$!

  echo -n "Waiting for daemon to be ready..."
  for _ in $(seq 1 30); do
    if curl -fsS http://127.0.0.1:3031/health >/dev/null 2>&1; then
      echo " ready!"
      break
    fi
    echo -n "."
    sleep 1
  done
  echo

  echo -n "Waiting for initial indexing..."
  for _ in $(seq 1 30); do
    local status files
    status="$(curl -fsS http://127.0.0.1:3031/status 2>/dev/null || echo "{}")"
    files="$(printf '%s' "$status" | python3 -c "import sys,json; print(json.load(sys.stdin).get('indexed_files', 0))" 2>/dev/null || echo "0")"
    if [ "$files" -gt 0 ]; then
      echo " $files files indexed!"
      break
    fi
    echo -n "."
    sleep 2
  done
  echo
}

# ── Search ───────────────────────────────────────────────────
run_search() {
  local query="${1:-semantic search}"
  header "Searching for: $query"
  echo

  local payload
  payload="$(python3 -c 'import json,sys; print(json.dumps({"query": sys.argv[1], "limit": 5}))' "$query")"

  cmd "curl -fsS http://127.0.0.1:3031/query -H 'Content-Type: application/json' -d '$payload'"
  echo
  curl -fsS "http://127.0.0.1:3031/query" \
    -H "Content-Type: application/json" \
    -d "$payload" \
    | python3 -m json.tool

  echo
  header "Done!"
  echo
  echo "  ✓ Daemon running on http://127.0.0.1:3031 (PID: ${DAEMON_PID:-unknown})"
  echo "  ✓ Configure AI tools: contextd connect --all"
}

# ── Main ─────────────────────────────────────────────────────
main() {
  local query="${1:-}"
  local workspace="${2:-.}"
  local config_file="contextd-playground.toml"

  echo
  echo -e "${BOLD}╔══════════════════════════════════════╗${NC}"
  echo -e "${BOLD}║       contextd Playground            ║${NC}"
  echo -e "${BOLD}╚══════════════════════════════════════╝${NC}"
  echo
  echo "  Local-first semantic search for AI agents"
  echo -e "  ${DIM}${GITHUB}${NC}"
  echo

  ensure_binary
  index_workspace "$workspace" "$config_file"

  if [ -z "$query" ]; then
    if [ -f "${workspace}/Cargo.toml" ]; then
      query="error handling"
    elif [ -f "${workspace}/package.json" ]; then
      query="async function"
    elif [ -f "${workspace}/main.py" ]; then
      query="import"
    else
      query="function"
    fi
  fi

  run_search "$query"

  echo -e "${DIM}(Press Ctrl+C to stop the daemon and clean up)${NC}"
  wait "${DAEMON_PID:-}" 2>/dev/null || true
}

main "$@"
