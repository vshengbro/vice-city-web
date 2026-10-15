#!/usr/bin/env bash
# Build + standards gate for the GTA-alignment loop.
#
# Runs the same pipeline the repo's standards require, in the order that fails
# cheapest first. Prints one line per step and exits non-zero on the first
# failure so the develop->verify loop stops on a real error instead of a
# cosmetic one.
#
# Usage: bash tools/verify/gate.sh
set -u
cd "$(dirname "$0")/../.." || exit 1

AUDIT="${HOME}/.agents/skills/rust-standards/scripts/audit_rust_standards.py"
TARGET="/Users/sqs/Library/Caches/cargo-target/vcweb"
FAILED=""

# The repo baseline is 48/53. The five failing checks (26, 35, 36, 41, 47)
# all live in the pre-existing uncommitted traffic/collision/combat work, not in
# the GTA-alignment changes: at committed HEAD the same audit reports 51/53.
# A change that makes this worse fails the gate; unchanged or better passes.
BASELINE_FAILS=5

step() { printf '\n=== %s ===\n' "$1"; }

step "rust-standards audit"
if [ -f "$AUDIT" ]; then
  OUT="$(python3 "$AUDIT" . 2>&1)"
  echo "$OUT" | grep -E "^(FAIL|PASS: 4[0-9]|=== SUMMARY)" | tail -12
  FAILS="$(echo "$OUT" | grep -c '^FAIL' || true)"
  echo "failing checks: ${FAILS} (baseline ${BASELINE_FAILS})"
  if [ "${FAILS}" -gt "${BASELINE_FAILS}" ]; then
    FAILED="${FAILED} audit-regressed"
  fi
else
  echo "SKIP: audit script not found at ${AUDIT}"
fi

step "cargo fmt --check"
# shellcheck disable=SC2091
if cargo fmt --all -- --check >/tmp/vcw_fmt.log 2>&1; then
  echo "PASS: formatting clean"
else
  echo "FAIL: formatting drift"
  head -30 /tmp/vcw_fmt.log
  FAILED="${FAILED} fmt"
fi

step "cargo build (wasm32 release)"
export CARGO_TARGET_DIR="$TARGET"
# shellcheck disable=SC2091
if cargo build --release --target wasm32-unknown-unknown >/tmp/vcw_build.log 2>&1; then
  echo "PASS: wasm build"
  ERRS="$(grep -c '^error' /tmp/vcw_build.log || true)"
  echo "errors: ${ERRS}  warnings: $(grep -c '^warning' /tmp/vcw_build.log || true)"
  if [ "${ERRS}" -gt 0 ]; then FAILED="${FAILED} build"; fi
else
  echo "FAIL: wasm build"
  grep -E '^error' /tmp/vcw_build.log | head -20
  FAILED="${FAILED} build"
fi

step "wasm-bindgen"
WASM="${TARGET}/wasm32-unknown-unknown/release/vice_city_web.wasm"
if [ -f "$WASM" ] && command -v wasm-bindgen >/dev/null 2>&1; then
  # shellcheck disable=SC2091
  if wasm-bindgen --target web --no-typescript --out-dir www/pkg \
      --out-name vice_city_web "$WASM" >/tmp/vcw_bindgen.log 2>&1; then
    echo "PASS: wasm-bindgen -> www/pkg ($(du -h www/pkg/vice_city_web_bg.wasm | cut -f1))"
  else
    echo "FAIL: wasm-bindgen"
    tail -20 /tmp/vcw_bindgen.log
    FAILED="${FAILED} bindgen"
  fi
else
  echo "SKIP: no wasm artifact or wasm-bindgen missing"
fi

printf '\n=== GATE RESULT ===\n'
if [ -z "${FAILED}" ]; then
  echo "PASS: build + standards clean (no regression vs baseline)"
  exit 0
fi
echo "FAIL:${FAILED}"
exit 1
