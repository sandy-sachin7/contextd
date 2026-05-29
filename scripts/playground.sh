#!/usr/bin/env bash
set -euo pipefail

# contextd Playground
# One-line: curl -fsSL https://raw.githubusercontent.com/sandy-sachin7/contextd/main/scripts/playground.sh | bash
# Or locally: bash scripts/playground.sh

REPO="sandy-sachin7/contextd"
GITHUB="https://github.com/$REPO"
RAW="https://raw.githubusercontent.com/$REPO/main"

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

cleanup() {
  if [ -n "${TMPDIR:-}" ] && [ -d "$TMPDIR" ]; then
    rm -rf "$TMPDIR"
  fi
}
trap cleanup EXIT

# ── Detect platform ──────────────────────────────────────────
detect_platform() {
  local arch
  arch="$(uname -m)"
  case "$(uname -s)" in
    Linux)  os="linux";;
    Darwin) os="macos";;
    *)      echo "Unsupported OS"; exit 1;;
  esac
  case "$arch" in
    x86_64|amd64) arch="x86_64";;
    aarch64|arm64) arch="aarch64";;
    *) echo "Unsupported arch: $arch"; exit 1;;
  esac
  echo "${arch}-${os}"
}

# ── Ensure contextd binary ───────────────────────────────────
ensure_binary() {
  if command -v contextd &>/dev/null; then
    info "contextd already installed at $(which contextd)"
    BINARY="contextd"
    return 0
  fi

  local platform
  platform="$(detect_platform)"
  local archive="contextd-${platform}.tar.gz"
  local url="${GITHUB}/releases/latest/download/${archive}"

  warn "contextd not found — downloading latest release..."
  cmd "curl -L $url | tar xz"

  TMPDIR="$(mktemp -d)"
  curl -fsSL "$url" -o "$TMPDIR/$archive"
  tar xzf "$TMPDIR/$archive" -C "$TMPDIR"

  local binary
  binary="$(find "$TMPDIR" -name "contextd" -type f 2>/dev/null | head -1)"

  if [ -z "$binary" ]; then
    echo "Failed to find contextd binary in extracted archive."
    exit 1
  fi

  chmod +x "$binary"

  # Install to ~/.local/bin or /usr/local/bin
  local install_dir="$HOME/.local/bin"
  if [ ! -d "$install_dir" ]; then
    mkdir -p "$install_dir"
  fi
  cp "$binary" "$install_dir/contextd"
  BINARY="$install_dir/contextd"
  export PATH="$install_dir:$PATH"

  info "Installed contextd to $BINARY"
  echo "  (add ~/.local/bin to your PATH if not already)"
}

# ── Init model (auto-download on first run) ──────────────────
init_model() {
  local model_dir="${1:-models}"

  if [ -f "$model_dir/model.onnx" ] && [ -f "$model_dir/tokenizer.json" ]; then
    info "Model files already exist in $model_dir"
    return 0
  fi

  header "Downloading default model (all-MiniLM-L6-v2)"
  echo "  (~87MB, downloaded once, cached in $model_dir/)"
  echo

  "$BINARY" setup
}

# ── Index workspace ──────────────────────────────────────────
index_workspace() {
  header "Indexing workspace"
  echo "  contextd will watch files in: ${1:-.}"
  echo

  local config_file="contextd-playground.toml"
  if [ ! -f "$config_file" ]; then
    cat > "$config_file" <<-EOF
			[server]
			host = "127.0.0.1"
			port = 3031

			[storage]
			db_path = "contextd-playground.db"
			model_path = "models"
			model_type = "all-minilm-l6-v2"

			[watch]
			paths = ["${1:-.}"]

			[watcher]
			debounce_ms = 500
			EOF
  fi

  info "Starting daemon (indexing in background)..."
  cmd "$BINARY --config $config_file daemon &"
  "$BINARY" --config "$config_file" daemon &
  DAEMON_PID=$!

  # Wait for daemon to become healthy
  echo -n "Waiting for daemon to be ready..."
  for i in $(seq 1 30); do
    if curl -sf http://127.0.0.1:3031/health >/dev/null 2>&1; then
      echo " ready!"
      break
    fi
    echo -n "."
    sleep 1
  done
  echo

  # Wait for initial indexing
  echo -n "Waiting for initial indexing..."
  for i in $(seq 1 30); do
    local status
    status="$(curl -sf http://127.0.0.1:3031/status 2>/dev/null || echo "{}")"
    local files
    files="$(echo "$status" | python3 -c "import sys,json; print(json.load(sys.stdin).get('indexed_files', 0))" 2>/dev/null || echo "0")"
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

  cmd "curl -s http://127.0.0.1:3031/query -H 'Content-Type: application/json' -d '{\"query\": \"$query\", \"limit\": 5}' | python3 -m json.tool"
  echo
  curl -s "http://127.0.0.1:3031/query" \
    -H "Content-Type: application/json" \
    -d "{\"query\": \"$query\", \"limit\": 5}" \
    | python3 -m json.tool 2>/dev/null || curl -s "http://127.0.0.1:3031/query" \
      -H "Content-Type: application/json" \
      -d "{\"query\": \"$query\", \"limit\": 5}"

  echo
  header "Done!"
  echo
  echo "  ✓ Daemon running on http://127.0.0.1:3031 (PID: ${DAEMON_PID:-unknown})"
  echo
  echo "  Try your own queries:"
  echo "    curl http://127.0.0.1:3031/query \\"
  echo "      -H 'Content-Type: application/json' \\"
  echo "      -d '{\"query\": \"your search\", \"limit\": 5}' | python3 -m json.tool"
  echo
  echo "  To stop the daemon:"
  echo "    kill ${DAEMON_PID:-0}"
  echo
  echo "  Or configure AI tools:"
  echo "    contextd connect --all"
  echo
  echo "  Install VSCode extension:"
  echo "    https://marketplace.visualstudio.com/items?itemName=sandy-sachin7.contextd-vscode"
}

# ── Cleanup handler for daemon ───────────────────────────────
cleanup_daemon() {
  if [ -n "${DAEMON_PID:-}" ] && kill -0 "$DAEMON_PID" 2>/dev/null; then
    warn "Stopping daemon (PID: $DAEMON_PID)"
    kill "$DAEMON_PID" 2>/dev/null
  fi
}

# ── Main ─────────────────────────────────────────────────────
main() {
  local query="${1:-}"
  local workspace="${2:-.}"

  echo
  echo "${BOLD}╔══════════════════════════════════════╗${NC}"
  echo "${BOLD}║       contextd Playground            ║${NC}"
  echo "${BOLD}╚══════════════════════════════════════╝${NC}"
  echo
  echo "  Local-first semantic search for AI agents"
  echo "  ${DIM}${GITHUB}${NC}"
  echo

  ensure_binary
  init_model "${workspace}/models"
  index_workspace "$workspace"

  trap cleanup_daemon EXIT

  if [ -z "$query" ]; then
    # Pick an intelligent default query based on the workspace language
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

  # Keep running until user Ctrl+C
  echo "${DIM}(Press Ctrl+C to stop the daemon and clean up)${NC}"
  wait "${DAEMON_PID:-}" 2>/dev/null || true
}

main "$@"
