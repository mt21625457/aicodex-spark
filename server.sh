#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

usage() {
  cat <<'EOF'
Usage:
  ./server.sh [options]

Options:
  -i, --ip <ip>            Bind IP address (default: 0.0.0.0)
  -p, --port <port>        Bind port (default: 9999)
  -e, --env-file <file>    Env file to load (default: .env if exists)
      --public-host <host> Public host for SERVER_PUBLIC_BASE_URL
      --public-scheme <s>  Public URL scheme (default: http)
  -h, --help               Show this help

Examples:
  ./server.sh
  ./server.sh --ip 192.168.1.20 --port 9999
  ./server.sh --ip 0.0.0.0 --port 9999 --public-host 192.168.1.20
EOF
}

BIND_IP="0.0.0.0"
PORT="9999"
ENV_FILE=".env"
PUBLIC_SCHEME="http"
PUBLIC_HOST=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    -i|--ip)
      BIND_IP="${2:-}"
      shift 2
      ;;
    -p|--port)
      PORT="${2:-}"
      shift 2
      ;;
    -e|--env-file)
      ENV_FILE="${2:-}"
      shift 2
      ;;
    --public-host)
      PUBLIC_HOST="${2:-}"
      shift 2
      ;;
    --public-scheme)
      PUBLIC_SCHEME="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if [[ -n "${ENV_FILE}" ]]; then
  if [[ -f "${ENV_FILE}" ]]; then
    echo "[server.sh] loading env file: ${ENV_FILE}"
    set -a
    # shellcheck disable=SC1090
    source "${ENV_FILE}"
    set +a
  else
    echo "[server.sh] env file not found, skip: ${ENV_FILE}"
  fi
fi

if [[ -z "${PUBLIC_HOST}" ]]; then
  if [[ "${BIND_IP}" == "0.0.0.0" ]]; then
    PUBLIC_HOST="localhost"
  else
    PUBLIC_HOST="${BIND_IP}"
  fi
fi

export SERVER_LISTEN_ADDR="${BIND_IP}:${PORT}"
export SERVER_PUBLIC_BASE_URL="${SERVER_PUBLIC_BASE_URL:-${PUBLIC_SCHEME}://${PUBLIC_HOST}:${PORT}}"

if [[ -z "${SERVER_DATABASE_URL:-}" && -z "${DATABASE_URL:-}" ]]; then
  echo "[server.sh] ERROR: SERVER_DATABASE_URL (or DATABASE_URL) is required" >&2
  exit 1
fi

if [[ -z "${ELECTRIC_URL:-}" ]]; then
  echo "[server.sh] ERROR: ELECTRIC_URL is required" >&2
  exit 1
fi

if [[ -z "${VIBEKANBAN_REMOTE_JWT_SECRET:-}" ]]; then
  echo "[server.sh] ERROR: VIBEKANBAN_REMOTE_JWT_SECRET is required" >&2
  exit 1
fi

echo "[server.sh] SERVER_LISTEN_ADDR=${SERVER_LISTEN_ADDR}"
echo "[server.sh] SERVER_PUBLIC_BASE_URL=${SERVER_PUBLIC_BASE_URL}"
echo "[server.sh] starting remote server..."

exec cargo run --manifest-path crates/remote/Cargo.toml --bin remote
