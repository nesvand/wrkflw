#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/lib.sh"

raw_model="${RALPH_MODEL:-GPT-5.3-Codex}"
variant="${RALPH_VARIANT:-high}"
verbose="${RALPH_VERBOSE:-0}"
repo_root="$(ralph_resolve_repo_root "$script_dir")"
model="$(ralph_resolve_model "$raw_model")"
cd "$repo_root"

if ! ralph_require_command opencode "opencode CLI not found in PATH"; then
  exit 1
fi

if ! ralph_require_command jq "jq is required"; then
  exit 1
fi

if [ "$#" -lt 1 ]; then
  echo "Usage: $0 <iterations>"
  exit 1
fi

iterations="$1"
if ! [[ "$iterations" =~ ^[1-9][0-9]*$ ]]; then
  echo "Error: <iterations> must be a positive integer"
  echo "Usage: $0 <iterations>"
  exit 1
fi

# jq filters for streaming OpenCode JSON events
stream_text='select(.type == "text") | .part.text // empty | gsub("\n"; "\r\n") | . + "\r\n\r\n"'
stream_verbose='if .type == "text" then ((.part.text // empty | gsub("\n"; "\r\n")) + "\r\n\r\n") elif .type == "tool_use" then ("[tool] " + (.part.tool // "unknown") + ": " + (.part.state.title // (.part.state.status // "running")) + "\r\n") elif .type == "step_start" then "[step] start\r\n" elif .type == "step_finish" then ("[step] finish (" + (.part.reason // "unknown") + ")\r\n") else empty end'

display_filter="$stream_text"
if ralph_is_verbose_enabled "$verbose"; then
  display_filter="$stream_verbose"
fi

done_marker='<promise>NO MORE TASKS</promise>'

for ((i=1; i<=iterations; i++)); do
  tmpfile=$(mktemp)

  message="$(ralph_build_message "$repo_root" "$script_dir")"

  opencode run \
    --dangerously-skip-permissions \
    --format json \
    --model "$model" \
    --variant "$variant" \
    "$message" \
  | tee "$tmpfile" \
  | jq --unbuffered -rj "$display_filter"

  if jq -e -s --arg marker "$done_marker" 'map(select(.type == "text") | (.part.text // "")) | any(contains($marker))' "$tmpfile" >/dev/null; then
    rm -f "$tmpfile"
    echo "Ralph complete after $i iterations."
    exit 0
  fi

  rm -f "$tmpfile"
done
