# Edge case robustness

## What to build

Unit tests and code hardening for all edge cases identified in the PRD: circular reference detection, malformed tag handling, empty paths, type mismatch fallback, max-depth safety limit, and transitive (nested) resolution.

No new model or API changes — purely hardening the resolver against unexpected input.

## Acceptance criteria

- [ ] Circular reference detection: resolver detects self-referencing and mutually-referencing cycles, leaves them unresolved, no infinite loop
- [ ] Malformed tag: `!reference "string"` (not an array) — left in place, no crash
- [ ] Empty path: `!reference []` — left unresolved, no crash
- [ ] Type mismatch: resolved value doesn't match consuming field type — replaced with path array, surfaced via validation issues
- [ ] Max-depth safety: resolution halts after 100 recursive steps
- [ ] Transitive resolution: resolved value containing further `!reference` tags is re-scanned and resolved
- [ ] Test fixture `reference-malformed.gitlab-ci.yml` exercising all malformed variants
- [ ] Unit tests in `crates/parser/src/reference_resolver.rs` for each edge case above

## Blocked by

- Issue #001 — Same-file `!reference` in rules (core tracer bullet)
- Issue #002 — External `!reference` fallback in rules
