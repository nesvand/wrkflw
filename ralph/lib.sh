#!/usr/bin/env bash

ralph_resolve_model() {
  case "$1" in
    GPT-5.3-Codex|gpt-5.3-codex)
      printf 'github-copilot/gpt-5.3-codex\n'
      ;;
    *)
      printf '%s\n' "$1"
      ;;
  esac
}

ralph_is_verbose_enabled() {
  case "$1" in
    1|true|TRUE|yes|YES|on|ON)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

ralph_require_command() {
  local command_name="$1"
  local error_message="$2"
  if ! command -v "$command_name" >/dev/null 2>&1; then
    printf '%s\n' "$error_message" >&2
    return 1
  fi
}

ralph_resolve_repo_root() {
  local script_dir="$1"
  if [ -n "${RALPH_REPO_ROOT:-}" ]; then
    printf '%s\n' "$RALPH_REPO_ROOT"
    return 0
  fi

  if git -C "$script_dir" rev-parse --show-toplevel >/dev/null 2>&1; then
    git -C "$script_dir" rev-parse --show-toplevel
    return 0
  fi

  (cd "$script_dir/.." && pwd)
}

ralph_resolve_prompt_file() {
  local script_dir="$1"
  local repo_root="$2"
  local prompt_file="${RALPH_PROMPT_FILE:-$script_dir/prompt.md}"
  if [[ "$prompt_file" = /* ]]; then
    printf '%s\n' "$prompt_file"
    return 0
  fi
  printf '%s/%s\n' "$repo_root" "$prompt_file"
}

ralph_read_issues() {
  local repo_root="$1"
  local issues_glob="${RALPH_ISSUES_GLOB:-docs/issues/*.md}"

  (
    cd "$repo_root" || exit 1
    shopt -s nullglob
    local files=( $issues_glob )
    if [ "${#files[@]}" -eq 0 ]; then
      printf 'No issues found\n'
      exit 0
    fi
    cat "${files[@]}"
  )
}

ralph_read_commits() {
  local repo_root="$1"
  local commit_count="${RALPH_COMMIT_COUNT:-5}"

  if ! [[ "$commit_count" =~ ^[1-9][0-9]*$ ]]; then
    commit_count=5
  fi

  git -C "$repo_root" log -n "$commit_count" --format="%H%n%ad%n%B---" --date=short 2>/dev/null || printf 'No commits found\n'
}

ralph_build_message() {
  local repo_root="$1"
  local script_dir="$2"
  local prompt_file
  local commits
  local issues
  local prompt

  prompt_file="$(ralph_resolve_prompt_file "$script_dir" "$repo_root")"
  if [ ! -f "$prompt_file" ]; then
    printf 'Prompt file not found: %s\n' "$prompt_file" >&2
    return 1
  fi

  commits="$(ralph_read_commits "$repo_root")"
  issues="$(ralph_read_issues "$repo_root")"
  prompt="$(cat "$prompt_file")"

  cat <<EOF
Previous commits:
$commits

Issues:
$issues

$prompt
EOF
}
