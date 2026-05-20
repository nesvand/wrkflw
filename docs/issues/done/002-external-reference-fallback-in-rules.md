# External `!reference` fallback in rules

## What to build

Adds "target not found" fallback to the resolver: when a `!reference` tag's path can't be found in the document tree, replace the tag with the inner path-array sequence (e.g., `[".rules", "except_config"]`). Adds the `Raw(serde_yaml::Value)` variant to the `Rule` enum so path arrays in rules fields deserialize without error.

End-to-end behavior: a `.gitlab-ci.yml` with `!reference [.rules, except_config]` where `.rules` is not defined in the file produces a valid `Pipeline` with `Rule::Raw(Value::Sequence(...))` values.

## Acceptance criteria

- [ ] Resolver replaces unresolved `!reference` tags with their inner path-array sequence when target not found in document tree
- [ ] `Rule` enum gains `Raw(serde_yaml::Value)` variant (untagged, tested first)
- [ ] Test fixture `reference-external.gitlab-ci.yml` with unresolvable `!reference` in `rules` (matches real-world pattern)
- [ ] Integration test asserts `Rule::Raw` is produced and parsing succeeds with no errors
- [ ] Parsed `Rule::Raw` preserves the full path segments for downstream use

## Blocked by

- Issue #001 — Same-file `!reference` in rules (core tracer bullet)
