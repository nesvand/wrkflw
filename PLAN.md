# GitLab CI `!reference` Tag Validation Implementation Plan

## Purpose

Add proper handling and resolution of GitLab's custom `!reference` YAML tag to
the existing CI YAML validator, which currently uses standard JSON Schema
validation that silently drops or misinterprets YAML tags.

## Background

- `!reference` is the **only** custom YAML tag GitLab defines
- Standard YAML features (anchors `&name`, aliases `*name`, merge `<<:`) are
  already handled by `serde_yaml` natively
- The current pipeline at `schema.rs` uses
  `serde_yaml::from_str::<serde_json::Value>` which drops tag information
- The typed deserialization at `gitlab.rs` into `Pipeline`/`Job` structs
  fails on fields typed as `Option<Vec<String>>` when a `!reference` tag is
  encountered
- The real-world target file uses `!reference` exclusively in `rules: Vec<Rule>`
  arrays (not `script`/`extends`), which is the primary bottleneck

---

## Stage 1 — Reference Resolver Module

**Goal:** Create a new module that resolves `!reference` tags within a single
YAML document tree.

### 1.1 Create `crates/parser/src/reference_resolver.rs`

```rust
pub fn resolve_references(doc: &serde_yaml::Value) -> serde_yaml::Value
```

### 1.2 Resolution Logic

1. Parse YAML to `serde_yaml::Value` (preserves tags natively)
2. Recursively walk the document tree with cycle detection
3. On encountering `Value::Tagged { tag, value }` where `tag == "!reference"`:
   a. Check if path is in current resolution chain → if so, leave unresolved
   b. Extract the inner `value` — expect a `Value::Sequence` of strings
   c. Walk the document tree following the path segments
   d. If target found → replace the entire `Tagged` node with the resolved value,
      then recurse into the resolved value for transitive resolution
   e. If target not found → replace with the inner sequence (path array) so
      downstream doesn't choke on `Value::Tagged`
4. Return the transformed document

### 1.3 Cycle Detection

- Maintain a `HashSet<Vec<String>>` tracking the current resolution chain
- Max depth safety limit of 100 to catch implementation bugs
- Cycle = encountering a path already in the current chain → leave unresolved

### 1.4 Edge Cases

- **Circular references**: Detect cycles during resolution and leave unresolved
- **Malformed reference**: `!reference "string"` instead of `!reference [...]` —
  leave as-is, don't crash
- **Empty path**: `!reference []` — leave unresolved
- **Nested references within referenced content**: Recurse into resolved values
  for transitive resolution
- **Type mismatch after resolution**: Replace with raw path array and surface
  via `ValidationResult` issues list

### 1.5 Files Touched

- `crates/parser/src/reference_resolver.rs` — **NEW**
- `crates/parser/src/lib.rs` — add `pub mod reference_resolver;`

---

## Stage 2 — Wire Resolver into Schema Validation

**Goal:** Use the resolved YAML before converting to JSON for schema
validation, so `!reference` tags don't produce invalid JSON.

### 2.1 Modify `crates/parser/src/schema.rs`

1. Extract JSON validation body into private `validate_json_value()` accepting
   `&serde_json::Value`
2. Public `validate_with_specific_schema()` calls it after parsing (unchanged
   API)
3. `parse_pipeline()` in `gitlab.rs` owns the full pipeline: parse →
   resolve → convert to JSON → call `validate_json_value()`

### 2.2 Old vs New Flow

```
Before: YAML → serde_json::Value (tags dropped) → JSON Schema validate
After:  YAML → serde_yaml::Value → resolve_references() → serde_json::Value → JSON Schema validate
```

### 2.3 Schema Cleanup

The `"!reference"` definitions in `gitlab-ci.json` are `$ref`-ed from
multiple locations. Leave them in place — they become inert dead code after
Stage 2, and removing them would require invasive schema surgery.

### 2.4 Files Touched

- `crates/parser/src/schema.rs` — extract `validate_json_value()`
- No schema file changes

---

## Stage 3 — Wire Resolver into Typed Deserialization

**Goal:** Resolve `!reference` tags before deserializing into `Pipeline`/`Job`
structs so typed fields don't fail on `Tagged` values.

### 3.1 Modify `crates/parser/src/gitlab.rs`

In `parse_pipeline()`:

1. Parse YAML to `serde_yaml::Value`
2. Call `resolve_references()` on it
3. Convert to `serde_json::Value` and call `validate_json_value()` (Stage 2)
4. Use `serde_yaml::from_value::<Pipeline>(resolved)` instead of
   `serde_yaml::from_str::<Pipeline>(&pipeline_content)`
5. Structural validation as before

### 3.2 Files Touched

- `crates/parser/src/gitlab.rs`

---

## Stage 4 — (Removed)

Originally proposed: widen `deserialize_string_or_vec`. After evaluation,
`deserialize_string_or_vec` fields (`script`, `before_script`, `after_script`,
`extends`) already work after Stage 3 — an unresolved `!reference` produces
a path array `["key", "field"]` which is a valid `Vec<String>`.

The real bottleneck is `rules: Vec<Rule>` — addressed in the next section.

---

## Stage 4B — Rule Enum with Raw Fallback

**Goal:** Handle unresolved `!reference` tags in `rules: Vec<Rule>` fields
(by far the most common pattern in practice).

### 4B.1 Convert `Rule` to Untagged Enum

In `crates/models/src/lib.rs`, replace the `Rule` struct with:

```rust
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum Rule {
    Structured {
        if_: Option<String>,
        when: Option<String>,
        variables: Option<HashMap<String, String>>,
    },
    Raw(serde_yaml::Value),
}
```

- Normal rule mappings `{if: ..., when: always}` → `Rule::Structured`
- Path arrays `[".rules", "except_config"]` → `Rule::Raw(Value::Sequence(...))`
- Affects both `Job.rules` and `Workflow.rules` automatically

### 4B.2 Files Touched

- `crates/models/src/lib.rs` — change `Rule` from struct to untagged enum

---

## Stage 5 — Test Fixtures & Integration Tests

**Goal:** Ensure the resolver, model, and validation pipeline work correctly
with real-world `!reference` usage.

### 5.1 Test Fixtures

Create in `tests/fixtures/gitlab-ci/`:

- `reference-simple.gitlab-ci.yml` — `!reference` within same file (define
  `.rules` as a hidden job, reference `.rules.*` in job `rules` arrays)
- `reference-external.gitlab-ci.yml` — `!reference [.rules, ...]` where `.rules`
  is not defined (mirrors real-world pattern); verifies `Rule::Raw` catch
- `reference-malformed.gitlab-ci.yml` — malformed `!reference` usages

### 5.2 Unit Tests

In `crates/parser/src/reference_resolver.rs`:

- `test_resolve_simple_reference()`
- `test_resolve_nested_path_reference()`
- `test_resolve_missing_target_graceful()` → produces path array
- `test_resolve_circular_reference_no_infinite_loop()`
- `test_resolve_malformed_tag_no_crash()`

### 5.3 Integration Tests

Add to `crates/parser/src/gitlab.rs` test module:

- `test_parse_pipeline_with_references()` — same-file `!reference` in rules,
  verify `Rule::Structured` is produced
- `test_validate_pipeline_with_external_references()` — unresolvable
  `!reference` in rules, verify `Rule::Raw` is produced and no crash

---

## Stage 6 — Validator Adjustments

**Goal:** Prevent false "extends undefined job" errors when `extends` uses
`!reference` to an external template.

### 6.1 Pre-Scan for Tagged Extends

Before running the resolver in `parse_pipeline()`:

1. Scan the raw parsed `serde_yaml::Value` for any job whose `extends` field
   contains a `Value::Tagged("!reference")`
2. Collect those job names into a set
3. Pass the set to `validate_extends()` in the validators crate
4. In `validate_extends()`, skip the "target exists" check for jobs in the set

### 6.2 Files Touched

- `crates/parser/src/gitlab.rs` — pre-scan before resolution
- `crates/validators/src/gitlab.rs` — skip check for tagged extends

---

## Dependency Graph

```
Stage 1 ──────► Stage 2 ──► Stage 6
(reference     (schema        (validator
 resolver)      integration)   pre-scan)
   │
   ├──────► Stage 4B ──► Stage 3
   │        (Rule enum     (typed
   │         change)        deserialization)
   │              │
   │              └──────► Stage 5
   │                       (tests)
   └──────────────────► Stage 5
```

All stages depend on Stage 1.
Stage 4B parallel with Stage 1 (different crate).
Stage 2 parallel with Stage 1 (different file).
Stage 3 depends on Stage 1 + Stage 4B.
Stage 6 depends on Stage 2.
Stage 5 depends on all preceding stages.

---

## File Manifest

| File | Action | Stage |
|------|--------|-------|
| `crates/parser/src/reference_resolver.rs` | Create | 1 |
| `crates/parser/src/lib.rs` | Edit | 1 |
| `crates/parser/src/schema.rs` | Edit | 2 |
| `crates/models/src/lib.rs` | Edit | 4B |
| `crates/parser/src/gitlab.rs` | Edit | 3, 6 |
| `crates/validators/src/gitlab.rs` | Edit | 6 |
| `tests/fixtures/gitlab-ci/reference-simple.gitlab-ci.yml` | Create | 5 |
| `tests/fixtures/gitlab-ci/reference-external.gitlab-ci.yml` | Create | 5 |
| `tests/fixtures/gitlab-ci/reference-malformed.gitlab-ci.yml` | Create | 5 |
