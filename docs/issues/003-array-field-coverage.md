# Array field coverage (script, before_script, after_script)

## What to build

Tests only — no code changes needed. After issues #001 and #002, `!reference` tags in `script`, `before_script`, `after_script`, `extends`, and `dependencies` already work: unresolved tags produce path-array sequences that deserialize correctly into `Vec<String>`. This slice adds test fixtures and integration tests to verify this explicitly.

End-to-end behavior: a `.gitlab-ci.yml` with `!reference` in array-typed fields parses without error and preserves the path segments in the produced strings.

## Acceptance criteria

- [ ] Test fixture with `!reference` in `script` (same-file resolvable and unresolvable)
- [ ] Test fixture with `!reference` in `before_script` and `after_script`
- [ ] Integration tests assert correct string values in deserialized arrays
- [ ] Test fixture with `!reference` in `extends` (array-shaped after resolution)
- [ ] Test fixture with `!reference` in `dependencies`

## Blocked by

- Issue #001 — Same-file `!reference` in rules (core tracer bullet)
