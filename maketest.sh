#!/bin/bash
set -euo pipefail

IN_PATH="test.ml"
OUT_PATH="tests/$1.test"
cp "$IN_PATH" "$OUT_PATH"
cargo run --bin test_runner -- --update "$OUT_PATH"