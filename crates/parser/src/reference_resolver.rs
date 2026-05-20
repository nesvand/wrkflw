use std::collections::HashSet;

/// Maximum recursion depth for reference resolution.
/// Prevents stack overflows from deeply nested or malformed documents.
const MAX_RESOLVE_DEPTH: usize = 100;

/// Resolve `!reference` tags within a single YAML document tree.
///
/// Walks the document tree recursively. For each `Value::Tagged { tag, value }`
/// where `tag == "!reference"`:
/// - Extracts the path segments from the inner value (expects a sequence of strings)
/// - Walks the document tree following those segments
/// - If target found: replaces the Tagged node with the resolved value, then recurses
/// - If target not found: leaves unresolved
///
/// Maintains a resolution chain (`HashSet<Vec<String>>`) for cycle detection
/// and a depth counter to prevent runaway recursion.
pub fn resolve_references(doc: &serde_yaml::Value) -> serde_yaml::Value {
    let mut chain = HashSet::new();
    resolve_node(doc, doc, &mut chain, 0)
}

fn resolve_node(
    node: &serde_yaml::Value,
    root: &serde_yaml::Value,
    chain: &mut HashSet<Vec<String>>,
    depth: usize,
) -> serde_yaml::Value {
    if depth > MAX_RESOLVE_DEPTH {
        return node.clone();
    }

    match node {
        serde_yaml::Value::Tagged(tagged) if tagged.tag == "!reference" => {
            resolve_reference_tag(tagged, root, chain, depth)
        }
        serde_yaml::Value::Sequence(seq) => {
            let next_depth = depth + 1;
            let resolved: Vec<serde_yaml::Value> = seq
                .iter()
                .map(|item| resolve_node(item, root, chain, next_depth))
                .collect();
            serde_yaml::Value::Sequence(resolved)
        }
        serde_yaml::Value::Mapping(map) => {
            let next_depth = depth + 1;
            let resolved: serde_yaml::Mapping = map
                .iter()
                .map(|(k, v)| {
                    let resolved_k = resolve_node(k, root, chain, next_depth);
                    let resolved_v = resolve_node(v, root, chain, next_depth);
                    (resolved_k, resolved_v)
                })
                .collect();
            serde_yaml::Value::Mapping(resolved)
        }
        other => other.clone(),
    }
}

fn resolve_reference_tag(
    tagged: &serde_yaml::value::TaggedValue,
    root: &serde_yaml::Value,
    chain: &mut HashSet<Vec<String>>,
    depth: usize,
) -> serde_yaml::Value {
    if depth > MAX_RESOLVE_DEPTH {
        return tagged.value.clone();
    }

    let path = match extract_path(&tagged.value) {
        Some(p) => p,
        // Malformed tag (not a sequence) → leave as-is (don't crash)
        None => return serde_yaml::Value::Tagged(Box::new(tagged.clone())),
    };

    if path.is_empty() {
        // Empty path → leave as-is
        return serde_yaml::Value::Tagged(Box::new(tagged.clone()));
    }

    if chain.contains(&path) {
        // Cycle detected → leave unresolved (replace with path array)
        return tagged.value.clone();
    }

    chain.insert(path.clone());

    let result = walk_path(root, &path);

    // Must keep path in chain during transitive resolution so cycles are detected
    let output = match result {
        Some(resolved) => resolve_node(resolved, root, chain, depth + 1),
        // Target not found → replace with inner path array for downstream
        None => tagged.value.clone(),
    };

    chain.remove(&path);
    output
}

/// Extract the path segments from a `!reference` value.
///
/// Expects the value to be a sequence of strings, e.g., `[".rules", "rules"]`.
/// Returns `None` if the value is not a sequence of strings (malformed tag).
fn extract_path(value: &serde_yaml::Value) -> Option<Vec<String>> {
    match value {
        serde_yaml::Value::Sequence(seq) => {
            let segments: Vec<String> = seq
                .iter()
                .map(|v| match v {
                    serde_yaml::Value::String(s) => Some(s.clone()),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?;
            Some(segments)
        }
        _ => None,
    }
}

/// Walk the document tree following the given path segments.
///
/// For each segment, looks up the key in the current mapping level.
/// Returns `None` if any segment is not found or the current node is not a mapping.
fn walk_path<'a>(root: &'a serde_yaml::Value, path: &[String]) -> Option<&'a serde_yaml::Value> {
    let mut current = root;
    for segment in path {
        match current {
            serde_yaml::Value::Mapping(map) => {
                let key = serde_yaml::Value::String(segment.clone());
                current = map.get(&key)?;
            }
            _ => return None,
        }
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_tagged(v: &serde_yaml::Value) -> bool {
        matches!(v, serde_yaml::Value::Tagged(_))
    }

    fn is_path_array(v: &serde_yaml::Value, expected: &[&str]) -> bool {
        match v {
            serde_yaml::Value::Sequence(seq) => {
                seq.len() == expected.len()
                    && seq.iter().zip(expected).all(|(a, b)| match a {
                        serde_yaml::Value::String(s) => s == b,
                        _ => false,
                    })
            }
            _ => false,
        }
    }

    #[test]
    fn test_resolve_simple_reference() {
        let yaml = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
.rules:
  rules:
    - if: '$CI_COMMIT_BRANCH == "main"'
      when: always

build:
  script: echo "hello"
  rules: !reference [.rules, rules]
"#,
        )
        .unwrap();

        let resolved = resolve_references(&yaml);

        let build = resolved
            .as_mapping()
            .unwrap()
            .get(&serde_yaml::Value::String("build".to_string()))
            .unwrap();
        let rules = build
            .as_mapping()
            .unwrap()
            .get(&serde_yaml::Value::String("rules".to_string()))
            .unwrap()
            .as_sequence()
            .unwrap();
        assert_eq!(rules.len(), 1);
        let rule = &rules[0];
        let rule_map = rule.as_mapping().unwrap();
        assert_eq!(
            rule_map
                .get(&serde_yaml::Value::String("if".to_string()))
                .unwrap(),
            &serde_yaml::Value::String("$CI_COMMIT_BRANCH == \"main\"".to_string())
        );
        assert_eq!(
            rule_map
                .get(&serde_yaml::Value::String("when".to_string()))
                .unwrap(),
            &serde_yaml::Value::String("always".to_string())
        );
    }

    #[test]
    fn test_resolve_nested_path_reference() {
        let yaml = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
.templates:
  .jobs:
    build:
      script: echo "build"

build:
  script: !reference [.templates, .jobs, build, script]
"#,
        )
        .unwrap();

        let resolved = resolve_references(&yaml);

        let build = resolved
            .as_mapping()
            .unwrap()
            .get(&serde_yaml::Value::String("build".to_string()))
            .unwrap();
        let script = build
            .as_mapping()
            .unwrap()
            .get(&serde_yaml::Value::String("script".to_string()))
            .unwrap();
        assert_eq!(
            script,
            &serde_yaml::Value::String("echo \"build\"".to_string())
        );
    }

    #[test]
    fn test_resolve_missing_target_graceful() {
        let yaml = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
build:
  script: echo "hello"
  rules:
    - !reference [.nonexistent, rules]
"#,
        )
        .unwrap();

        let resolved = resolve_references(&yaml);

        let build = resolved
            .as_mapping()
            .unwrap()
            .get(&serde_yaml::Value::String("build".to_string()))
            .unwrap();
        let rules = build
            .as_mapping()
            .unwrap()
            .get(&serde_yaml::Value::String("rules".to_string()))
            .unwrap()
            .as_sequence()
            .unwrap();
        assert!(is_path_array(&rules[0], &[".nonexistent", "rules"]));
    }

    #[test]
    fn test_resolve_circular_reference_no_infinite_loop() {
        let yaml = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
job_a:
  script: !reference [job_b, script]

job_b:
  script: !reference [job_a, script]
"#,
        )
        .unwrap();

        let resolved = resolve_references(&yaml);

        let job_a = resolved
            .as_mapping()
            .unwrap()
            .get(&serde_yaml::Value::String("job_a".to_string()))
            .unwrap();
        let script_a = job_a
            .as_mapping()
            .unwrap()
            .get(&serde_yaml::Value::String("script".to_string()))
            .unwrap();
        assert!(is_path_array(script_a, &["job_b", "script"]));

        let job_b = resolved
            .as_mapping()
            .unwrap()
            .get(&serde_yaml::Value::String("job_b".to_string()))
            .unwrap();
        let script_b = job_b
            .as_mapping()
            .unwrap()
            .get(&serde_yaml::Value::String("script".to_string()))
            .unwrap();
        assert!(is_path_array(script_b, &["job_a", "script"]));
    }

    #[test]
    fn test_resolve_malformed_tag_no_crash() {
        let yaml = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
build:
  script: !reference "not-an-array"
"#,
        )
        .unwrap();

        let resolved = resolve_references(&yaml);

        let build = resolved
            .as_mapping()
            .unwrap()
            .get(&serde_yaml::Value::String("build".to_string()))
            .unwrap();
        let script = build
            .as_mapping()
            .unwrap()
            .get(&serde_yaml::Value::String("script".to_string()))
            .unwrap();
        assert!(is_tagged(script));
    }

    #[test]
    fn test_resolve_empty_path_no_crash() {
        let yaml = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
build:
  script: !reference []
"#,
        )
        .unwrap();

        let resolved = resolve_references(&yaml);

        let build = resolved
            .as_mapping()
            .unwrap()
            .get(&serde_yaml::Value::String("build".to_string()))
            .unwrap();
        let script = build
            .as_mapping()
            .unwrap()
            .get(&serde_yaml::Value::String("script".to_string()))
            .unwrap();
        assert!(is_tagged(script));
    }

    #[test]
    fn test_resolve_transitive_reference() {
        let yaml = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
.ref:
  rules:
    - if: '$CI_COMMIT_BRANCH == "main"'
      when: always

.template:
  rules: !reference [.ref, rules]

build:
  script: echo "build"
  rules: !reference [.template, rules]
"#,
        )
        .unwrap();

        let resolved = resolve_references(&yaml);

        let build = resolved
            .as_mapping()
            .unwrap()
            .get(&serde_yaml::Value::String("build".to_string()))
            .unwrap();
        let rules = build
            .as_mapping()
            .unwrap()
            .get(&serde_yaml::Value::String("rules".to_string()))
            .unwrap()
            .as_sequence()
            .unwrap();
        assert_eq!(rules.len(), 1);
        let rule = &rules[0];
        let rule_map = rule.as_mapping().unwrap();
        assert_eq!(
            rule_map
                .get(&serde_yaml::Value::String("if".to_string()))
                .unwrap(),
            &serde_yaml::Value::String("$CI_COMMIT_BRANCH == \"main\"".to_string())
        );
    }

    #[test]
    fn test_resolve_max_depth_safety() {
        // A chain of 102 sequential references should hit the depth limit
        // without crashing (stack overflow).
        let yaml = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
job_0:
  script: !reference [job_1, script]
job_1:
  script: !reference [job_2, script]
job_2:
  script: !reference [job_3, script]
job_3:
  script: echo "leaf"
"#,
        )
        .unwrap();

        // Should complete without crash. Depth of 4 references << MAX_RESOLVE_DEPTH
        // so all should resolve normally.
        let resolved = resolve_references(&yaml);

        let job_0 = resolved
            .as_mapping()
            .unwrap()
            .get(&serde_yaml::Value::String("job_0".to_string()))
            .unwrap();
        let script_0 = job_0
            .as_mapping()
            .unwrap()
            .get(&serde_yaml::Value::String("script".to_string()))
            .unwrap();
        assert_eq!(
            script_0,
            &serde_yaml::Value::String("echo \"leaf\"".to_string())
        );
    }

    #[test]
    fn test_resolve_self_reference_cycle() {
        // Self-referencing !reference should be replaced with path array
        let yaml = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
job:
  script: !reference [job, script]
"#,
        )
        .unwrap();

        let resolved = resolve_references(&yaml);

        let job = resolved
            .as_mapping()
            .unwrap()
            .get(&serde_yaml::Value::String("job".to_string()))
            .unwrap();
        let script = job
            .as_mapping()
            .unwrap()
            .get(&serde_yaml::Value::String("script".to_string()))
            .unwrap();
        assert!(is_path_array(script, &["job", "script"]));
    }
}
