# Ralph Scripts

Small helpers for running OpenCode against repo issues.

## Files

- `once.sh`: run a single OpenCode pass.
- `afk.sh`: run multiple passes until the model prints `<promise>NO MORE TASKS</promise>`.
- `prompt.md`: base instructions appended to each run.
- `lib.sh`: shared helpers used by both scripts.

## Requirements

- `opencode` in `PATH`
- `jq` in `PATH` (only for `afk.sh`)
- Bash

## Usage

From the repository root:

```bash
./ralph/once.sh
./ralph/afk.sh 5
```

## Environment variables

- `RALPH_MODEL` (default: `GPT-5.3-Codex`)
- `RALPH_VARIANT` (default: `high`)
- `RALPH_VERBOSE` (default: `0`; truthy enables tool/step stream output in `afk.sh`)
- `RALPH_REPO_ROOT` (optional repo path override)
- `RALPH_PROMPT_FILE` (optional prompt file path; absolute or repo-relative)
- `RALPH_ISSUES_GLOB` (default: `docs/issues/*.md`)
- `RALPH_COMMIT_COUNT` (default: `5`)

## Reusing in other projects

Copy `ralph/` into another repo and then adjust `prompt.md` and issue file layout as needed.

```bash
RALPH_ISSUES_GLOB="work-items/*.md" ./ralph/once.sh
```
