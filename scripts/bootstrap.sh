#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if ! command -v rustup >/dev/null 2>&1; then
  echo "rustup is not installed; installing non-interactively."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

rustup toolchain install nightly --profile minimal --component rustfmt --component clippy

python3 -m pip install --user --upgrade pandas scipy matplotlib numpy

python3 - <<'PY'
import importlib.util
missing = [m for m in ("pandas", "numpy", "scipy", "matplotlib") if importlib.util.find_spec(m) is None]
if missing:
    raise SystemExit(f"Missing Python analysis modules: {missing}")
print("Python analysis prerequisites OK.")
PY

echo "Bootstrap complete."
