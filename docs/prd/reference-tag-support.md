# PRD: GitLab CI `!reference` YAML Tag Support

## Problem Statement

The wrkflw CI YAML validator silently drops GitLab's `!reference` YAML tag during JSON Schema validation, and the typed deserialization into `Pipeline`/`Job` structs fails when encountering tagged values in typed fields like `rules`. This makes it impossible to validate or run real-world `.gitlab-ci.yml` files that use `!reference` — which is standard practice across GitLab CI pipelines.

## Solution

Add a reference tag resolver that walks the parsed YAML tree before both schema validation and typed deserialization. The resolver replaces `!reference` tags with their resolved values (for same-file targets) or a path-array fallback (for external targets). The typed model is widened at the `Rule` type to preserve unresolved references as raw values rather than failing.

The resolution step is invisible to existing consumers of the validation API — same inputs, same output types, just correct handling of `!reference` tags.

## User Stories

1. As a developer validating a GitLab CI config, I want `!reference` tags in `rules` fields to resolve correctly for same-file targets, so that validation succeeds on valid pipelines.
2. As a developer validating a GitLab CI config, I want `!reference` tags to external include files (the most common pattern) to not crash validation, so that I can validate files that reference shared templates.
3. As a developer running a GitLab CI pipeline through the runner, I want jobs with `!reference` in `rules` to parse correctly, so that the pipeline can execute.
4. As a developer using the tool, I want unresolved `!reference` tags to be preserved in the parsed model as raw data, so that downstream consumers (runtime, custom validators) can still inspect or process them.
5. As a developer writing a pipeline with malformed `!reference` usage, I want a descriptive validation error rather than a parser crash, so that I can fix my configuration.
6. As a developer with a pipeline that has circular `!reference` resolution chains, I want the resolver to detect and break the cycle gracefully, so that validation doesn't hang or stack-overflow.
7. As a developer using `extends: !reference` to inherit from an external template, I want the validator to not report false "extends undefined job" errors, so that valid configurations pass structural validation.
8. As a developer with mixed `!reference` and plain values in a `rules` array, I want both to be handled correctly, so that partial adoption of `!reference` works.
9. As a developer with a pipeline that uses nested `!reference` tags (a reference whose resolved value itself contains references), I want transitive resolution to resolve the full chain, so that deeply nested references work correctly.
10. As a developer with a pipeline that uses `!reference` in `script`, `before_script`, or `after_script` fields pointing to external files, I want deserialization to handle the path-array fallback gracefully, so that the pipeline model is populated without data loss.
11. As a developer revisiting this feature in 6 months, I want the decision to use pre-resolution (rather than serde visitors) documented, so that I understand why this approach was taken.
12. As a developer adding new GitLab CI features to the model, I want `Rule` to remain a typed struct for normal usage, so that I get compile-time guarantees for structured rule fields.

## Implementation Decisions

### Module: Reference Resolver (new, deep module)

A tree-walking resolver that operates on `serde_yaml::Value` before any type-level interpretation. Single public function:

```
resolve_references(doc: &serde_yaml::Value) -> serde_yaml::Value
```

- Interleaved walk-and-resolve: when a `Value::Tagged("!reference")` is found, immediately replace it with the resolved value and recurse into the result for transitive references.
- Cycle detection via a `HashSet` of path chains tracked through the recursion stack.
- Max-depth safety limit of 100.
- Fallback behavior for type mismatches (resolved value doesn't match expected field type): replace with raw path-array sequence and surface via validation issues.
- Malformed tags (non-array values, empty paths, string values): leave in place, don't crash.

### Module: Schema Validation (modified, shallow)

Extract the JSON validation body from the public `validate_with_specific_schema()` into a private `validate_json_value()` helper that accepts a pre-parsed `&serde_json::Value`. The public API remains unchanged. The parser owns the full orchestration pipeline: parse → resolve → convert to JSON → validate.

### Module: Pipeline Parser (modified, medium)

`parse_pipeline()` becomes the single owner of the parse→resolve→validate→deserialize pipeline. After Stage 1 resolver is applied to the `serde_yaml::Value`, the resolved tree feeds both schema validation (via JSON conversion) and typed deserialization (via `serde_yaml::from_value`). Also adds a pre-scan for `!reference` tags in `extends` fields before resolution, to preserve metadata for the validator stage.

### Module: Pipeline Model (modified, shallow)

`Rule` changes from a struct to an untagged enum:

```rust
enum Rule {
    Structured { if_, when, variables },
    Raw(serde_yaml::Value),
}
```

Normal rule mappings deserialize as `Structured`; unresolved `!reference` path arrays fall through to `Raw`. Affects both `Job.rules` and `Workflow.rules` automatically. All other model types remain unchanged — array fields (`script`, `before_script`, `after_script`, `extends`) already work with the path-array fallback after Stage 3.

### Module: Pipeline Validator (modified, shallow)

The extends-validity checker skips jobs whose `extends` was originally a `!reference` tag (detected via the pre-scan in the parser). Prevents false "extends undefined job" errors for external template references.

### Schema files

The existing `"!reference"` definitions in the embedded JSON Schema are left in place. They become inert dead code after the resolver runs (the validator never sees raw `!reference` values). Removing them would require invasive edits to `$ref` chains in a 3000-line auto-generated schema.

### Execution Order

1. Stage 1 (Reference Resolver) + Stage 4B (Rule enum) + Stage 2 (Schema refactor) — parallel, different crates
2. Stage 3 (Parser wiring) + Stage 6 (Validator adjustments) — parallel after dependencies met
3. Stage 5 (Tests) — after all code changes

## Testing Decisions

### Testing philosophy

Tests should validate external behavior — observable outcomes of parsing, validating, and running pipelines — not internal implementation details of the resolver. A resolver test asserts "given this YAML input, the parsed model has these specific values," not "the resolver called walk() 3 times."

### Modules under test

**Reference Resolver** — unit tests for all edge cases:
- Simple same-file reference resolution
- Nested path references (multi-segment keys)
- Missing target → graceful path-array fallback
- Circular reference → no infinite loop
- Malformed tag → no crash
- Transitive resolution (nested `!reference` in resolved value)
- Empty path → left unresolved

**Pipeline Parser** — integration tests with real YAML fixtures:
- Same-file `!reference` in `rules` → `Rule::Structured` produced
- External `!reference` in `rules` → `Rule::Raw` produced, no crash
- Mixed `!reference` and plain rules → both handled
- `!reference` in script/before_script/after_script → deserialized correctly

### Prior art

Existing tests in `crates/parser/src/gitlab.rs` (`test_parse_simple_pipeline`) and `crates/parser/src/workflow.rs` (14 tests) follow the pattern of loading fixture YAML, calling the parse function, and asserting on the resulting model. New tests should follow the same fixture-loading pattern. Existing fixtures live in `tests/fixtures/gitlab-ci/`.

## Out of Scope

- **Cross-file resolution.** Resolving `!reference` tags whose targets exist in included files (via `include:`) is explicitly out of scope. The resolver treats these as unresolvable and preserves them as path-array fallbacks. Runtime resolution of includes is a separate feature.
- **GitLab schema updates.** The embedded `gitlab-ci.json` is a snapshot; updating it to match the latest GitLab schema is out of scope.
- **Runner evaluation of `!reference`.** The runtime is not being modified to resolve `!reference` during pipeline execution — this PRD covers parse-time handling only.
- **New `!reference` locations.** Only the documented GitLab `!reference` locations (rules, script, before_script, after_script, extends, dependencies) are handled. Other fields with `!reference` are covered by the general type-mismatch fallback.
- **Schema definition cleanup.** The inert `"!reference"` definitions in the schema are left in place.
- **GitHub Actions parity.** This is GitLab CI only.

## Further Notes

- The ADR for the pre-resolution approach is at `docs/adr/0001-pre-resolve-reference-tags.md`.
- The domain glossary is at `CONTEXT.md`.
- The real-world target file that motivated this work is at `$UI/frontend/.gitlab-ci.yml` — it uses `!reference` exclusively in `rules` arrays (~40 occurrences), all pointing to external `.rules.*` templates in included files. This is the primary validation target.
- `serde_yaml` 0.9 is already in the dependency tree and natively supports `Value::Tagged`, which is the foundation of the resolver approach.
