# wrkflw Domain Glossary

## Terms

### reference tag
A YAML `!reference` tag — GitLab's custom tag syntax that acts as a path-based pointer to another part of the document. Written as `!reference [job_name, field]`. Distinguished from YAML anchors/aliases (`&name`/`*name`) which serde_yaml handles natively.

### extends directive
The GitLab `extends: job_name` keyword that merges another job's configuration into the current job. Not a YAML tag.

### reference resolution cycle
A cycle formed when transitively resolving `!reference` tags — e.g., job A's script contains `!reference [B, script]` and job B's script contains `!reference [A, script]`. Distinguished from extends cycles (already detected by validators).

### external reference
A `!reference` tag whose target exists in an included file (via `include:`) rather than the current document. Cannot be resolved at parse time.
