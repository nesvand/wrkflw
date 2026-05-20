# `!reference` in `extends` + validator adjustments

## What to build

Adds a pre-scan step in `parse_pipeline()` that identifies jobs whose `extends` field contains a `Value::Tagged("!reference")` before the resolver runs. Passes this metadata to the structural validator, which skips the "extends undefined job" check for those jobs. Prevents false validation errors when `extends: !reference [.templates, build_job]` points to an external template.

End-to-end behavior: a `.gitlab-ci.yml` with `extends: !reference [.templates, build_job]` where the template is not in the current file passes structural validation with no "undefined" error.

## Acceptance criteria

- [ ] Pre-scan in parser collects set of job names with tagged `extends` values before resolution
- [ ] Metadata flows to the structural validator (`validate_extends` or equivalent)
- [ ] Validator skips "extends target exists" check for jobs in the tagged set
- [ ] Test fixture with `extends: !reference` to an undeclared target
- [ ] Integration test asserts no false "extends undefined" error
- [ ] Regular `extends: job_name` (untagged) still validates as before

## Blocked by

- Issue #001 — Same-file `!reference` in rules (core tracer bullet)
- Issue #002 — External `!reference` fallback in rules
