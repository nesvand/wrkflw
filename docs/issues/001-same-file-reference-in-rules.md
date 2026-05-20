# Same-file `!reference` in rules (core tracer bullet)

## What to build

The thinnest vertical slice through the full validation pipeline. Creates the reference tag resolver with same-file resolution, converts the `Rule` model type to an untagged enum with `Structured` variant, extracts the JSON validation helper from the schema module, and wires `parse_pipeline()` to own the full parse→resolve→validate→deserialize flow.

End-to-end behavior: a `.gitlab-ci.yml` with `!reference [.rules, build]` where `.rules` is defined in the same file produces a valid `Pipeline` with `Rule::Structured` values. No crash, no dropped tags, no schema validation errors.

## Acceptance criteria

- [ ] `resolve_references()` exists in `crates/parser/src/reference_resolver.rs` and resolves same-file `!reference` tags by walking the document tree
- [ ] `Rule` is an untagged enum with `Structured { if_, when, variables }` variant (no `Raw` variant yet)
- [ ] `validate_json_value()` helper extracted in `crates/parser/src/schema.rs` — accepts `&serde_json::Value`
- [ ] `parse_pipeline()` uses the new flow: parse `serde_yaml::Value` → resolve → convert to JSON → validate → deserialize
- [ ] Test fixture `reference-simple.gitlab-ci.yml` with same-file `!reference` in `rules`
- [ ] Integration test asserts correct `Rule::Structured` values after parsing
- [ ] Existing tests (simple pipeline, advanced pipeline, invalid) still pass unchanged

## Blocked by

None — can start immediately
