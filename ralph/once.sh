#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/lib.sh"

raw_model="${RALPH_MODEL:-GPT-5.3-Codex}"
variant="${RALPH_VARIANT:-high}"
repo_root="$(ralph_resolve_repo_root "$script_dir")"
model="$(ralph_resolve_model "$raw_model")"
cd "$repo_root"

if ! ralph_require_command opencode "opencode CLI not found in PATH"; then
  exit 1
fi

message="$(ralph_build_message "$repo_root" "$script_dir")"

opencode run \
  --dangerously-skip-permissions \
  --model "$model" \
  --variant "$variant" \
  "$message"
