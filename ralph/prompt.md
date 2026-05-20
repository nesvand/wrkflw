# ISSUES

Local issue files from `docs/issues/` are provided at start of context. Parse them to understand the open issues.

You will work on the AFK issues only, not the HITL ones.

You've also been passed a file containing the last few commits. Review these to understand what work has been done.

If all AFK tasks are complete, output `<promise>NO MORE TASKS</promise>`.

# TASK SELECTION

Pick the next task. Prioritize tasks in this order:

1. Critical bugfixes
2. Development infrastructure

Getting development infrastructure like tests and types and dev scripts ready is an important precursor to building features.

3. Tracer bullets for new features

Tracer bullets are small slices of functionality that go through all layers of the system, allowing you to test and validate your approach early. Build a tiny, end-to-end slice of the feature first, then expand it out.

4. Polish and quick wins
5. Refactors

# IMPLEMENTATION

Use test-driven development principles from the repository's `tdd` skill.

# FEEDBACK LOOPS

Before committing, run the feedback loops:

- `cargo fmt` to format code
- `cargo clippy --workspace --all-features -- -D warnings` for linting
- `cargo test --workspace --all-features` to run tests
- `cargo build --workspace --all-features` as a build smoke check
- Run `regenerate_index` to keep INDEX.md current

# COMMIT

Make a git commit. The commit message must:

1. Include key decisions made
2. Include files changed
3. Blockers or notes for next iteration

# THE ISSUE

If the task is complete, move the issue file to `docs/issues/done/`.

If the task is not complete, add a note to the issue file with what was done.

# FINAL RULES

ONLY WORK ON A SINGLE TASK.
