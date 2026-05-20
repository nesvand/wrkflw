use serde_yaml::Value;
use std::fs;
use std::path::Path;

pub struct ResolvedPipeline {
    pub merged_yaml: Value,
    pub has_unresolved_includes: bool,
    pub failed_local_includes: Vec<String>,
}

/// Resolve `include` directives in a GitLab CI YAML document.
///
/// For each `include` entry:
/// - `local:` — resolved relative to the pipeline file directory, read,
///   parsed, and merged into the root mapping (shallow merge, later includes
///   override earlier ones). If the file cannot be read or its YAML is
///   invalid, the path is recorded in `failed_local_includes`.
/// - `project:` / `remote:` / `template:` / `component:` / `artifact:` —
///   tracked as unresolved. The caller can use this to skip "undefined job"
///   checks in the structural validator.
///
/// Include-level `rules` are ignored — we always include the content,
/// which is the safe side for validation (wider scope, no false negatives).
pub fn resolve_includes(yaml: &Value, pipeline_path: &Path) -> ResolvedPipeline {
    let root_map = match yaml.as_mapping() {
        Some(m) => m,
        None => {
            return ResolvedPipeline {
                merged_yaml: yaml.clone(),
                has_unresolved_includes: false,
                failed_local_includes: Vec::new(),
            };
        }
    };

    let include_key = Value::String("include".to_string());
    let include_value = match root_map.get(&include_key) {
        Some(v) => v,
        None => {
            return ResolvedPipeline {
                merged_yaml: yaml.clone(),
                has_unresolved_includes: false,
                failed_local_includes: Vec::new(),
            };
        }
    };

    let includes: Vec<&Value> = match include_value {
        Value::Sequence(seq) => seq.iter().collect(),
        Value::String(_) => vec![include_value],
        Value::Mapping(_) => vec![include_value],
        _ => {
            return ResolvedPipeline {
                merged_yaml: yaml.clone(),
                has_unresolved_includes: false,
                failed_local_includes: Vec::new(),
            };
        }
    };

    let pipeline_dir = pipeline_path.parent().unwrap_or(Path::new("."));
    let mut merged = yaml.clone();
    let merged_map = merged.as_mapping_mut().unwrap();
    let mut has_unresolved = false;
    let mut failed_local_includes: Vec<String> = Vec::new();

    for include_item in &includes {
        match include_item.as_mapping() {
            Some(include_map) => {
                let local_key = Value::String("local".to_string());
                let project_key = Value::String("project".to_string());
                let remote_key = Value::String("remote".to_string());
                let template_key = Value::String("template".to_string());
                let component_key = Value::String("component".to_string());
                let artifact_key = Value::String("artifact".to_string());

                if let Some(local_val) = include_map.get(&local_key) {
                    if let Some(local_path) = local_val.as_str() {
                        let include_path = pipeline_dir.join(local_path);
                        match fs::read_to_string(&include_path) {
                            Ok(content) => {
                                if let Ok(include_yaml) = serde_yaml::from_str::<Value>(&content) {
                                    if let Some(include_map_yaml) = include_yaml.as_mapping() {
                                        for (k, v) in include_map_yaml {
                                            merged_map.insert(k.clone(), v.clone());
                                        }
                                    }
                                } else {
                                    has_unresolved = true;
                                    failed_local_includes.push(local_path.to_string());
                                }
                            }
                            Err(_) => {
                                has_unresolved = true;
                                failed_local_includes.push(local_path.to_string());
                            }
                        }
                    }
                    continue;
                }

                if include_map.contains_key(&project_key)
                    || include_map.contains_key(&remote_key)
                    || include_map.contains_key(&template_key)
                    || include_map.contains_key(&component_key)
                    || include_map.contains_key(&artifact_key)
                {
                    has_unresolved = true;
                    continue;
                }
            }
            None => {
                if let Some(path_str) = include_item.as_str() {
                    let include_path = pipeline_dir.join(path_str);
                    match fs::read_to_string(&include_path) {
                        Ok(content) => {
                            if let Ok(include_yaml) = serde_yaml::from_str::<Value>(&content) {
                                if let Some(include_map_yaml) = include_yaml.as_mapping() {
                                    for (k, v) in include_map_yaml {
                                        merged_map.insert(k.clone(), v.clone());
                                    }
                                }
                            } else {
                                has_unresolved = true;
                                failed_local_includes.push(path_str.to_string());
                            }
                        }
                        Err(_) => {
                            has_unresolved = true;
                            failed_local_includes.push(path_str.to_string());
                        }
                    }
                }
                continue;
            }
        }
    }

    ResolvedPipeline {
        merged_yaml: merged,
        has_unresolved_includes: has_unresolved,
        failed_local_includes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn write_yaml(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_resolve_local_include() {
        let dir = TempDir::new().unwrap();
        write_yaml(
            dir.path(),
            ".gitlab/ci/templates/.deps.yml",
            r#"
.dependencies_cache:
  cache:
    key: node_modules
    paths:
      - node_modules/
"#,
        );

        let main_yaml = serde_yaml::from_str::<Value>(&format!(
            r#"
stages:
  - build

include:
  - local: .gitlab/ci/templates/.deps.yml

build:
  stage: build
  script: echo "building"
  extends: .dependencies_cache
"#
        ))
        .unwrap();

        let resolved = resolve_includes(&main_yaml, &dir.path().join(".gitlab-ci.yml"));

        // After merge, .dependencies_cache should be available
        let merged_map = resolved.merged_yaml.as_mapping().unwrap();
        let deps_cache_key = Value::String(".dependencies_cache".to_string());
        assert!(merged_map.contains_key(&deps_cache_key));
        assert!(!resolved.has_unresolved_includes);
    }

    #[test]
    fn test_resolve_multiple_local_includes() {
        let dir = TempDir::new().unwrap();
        write_yaml(
            dir.path(),
            "templates/cache.yml",
            r#"
.dependencies_cache:
  cache:
    key: node_modules
"#,
        );
        write_yaml(
            dir.path(),
            "jobs/unit.yml",
            r#"
unit-test:
  stage: test
  script: npm test
"#,
        );

        let main_yaml = serde_yaml::from_str::<Value>(&format!(
            r#"
stages:
  - test

include:
  - local: templates/cache.yml
  - local: jobs/unit.yml
"#
        ))
        .unwrap();

        let resolved = resolve_includes(&main_yaml, &dir.path().join(".gitlab-ci.yml"));
        let merged_map = resolved.merged_yaml.as_mapping().unwrap();

        assert!(merged_map.contains_key(&Value::String(".dependencies_cache".to_string())));
        assert!(merged_map.contains_key(&Value::String("unit-test".to_string())));
        assert!(!resolved.has_unresolved_includes);
    }

    #[test]
    fn test_project_include_marked_unresolved() {
        let main_yaml = serde_yaml::from_str::<Value>(&format!(
            r#"
stages:
  - build

include:
  - project: technology/templates
    file: build.yml

build:
  stage: build
  script: echo "building"
"#
        ))
        .unwrap();

        let resolved = resolve_includes(&main_yaml, Path::new(".gitlab-ci.yml"));

        assert!(resolved.has_unresolved_includes);
    }

    #[test]
    fn test_remote_include_marked_unresolved() {
        let main_yaml = serde_yaml::from_str::<Value>(&format!(
            r#"
include:
  - remote: https://example.com/templates.yml

build:
  script: echo "building"
"#
        ))
        .unwrap();

        let resolved = resolve_includes(&main_yaml, Path::new(".gitlab-ci.yml"));
        assert!(resolved.has_unresolved_includes);
    }

    #[test]
    fn test_template_include_marked_unresolved() {
        let main_yaml = serde_yaml::from_str::<Value>(&format!(
            r#"
include:
  - template: Jobs/SAST.gitlab-ci.yml

build:
  script: echo "building"
"#
        ))
        .unwrap();

        let resolved = resolve_includes(&main_yaml, Path::new(".gitlab-ci.yml"));
        assert!(resolved.has_unresolved_includes);
    }

    #[test]
    fn test_component_include_marked_unresolved() {
        let main_yaml = serde_yaml::from_str::<Value>(&format!(
            r#"
include:
  - component: example.com/my-component@1.0

build:
  script: echo "building"
"#
        ))
        .unwrap();

        let resolved = resolve_includes(&main_yaml, Path::new(".gitlab-ci.yml"));
        assert!(resolved.has_unresolved_includes);
    }

    #[test]
    fn test_artifact_include_marked_unresolved() {
        let main_yaml = serde_yaml::from_str::<Value>(&format!(
            r#"
include:
  - artifact: pipeline-config.yml
    job: generate-config

build:
  script: echo "building"
"#
        ))
        .unwrap();

        let resolved = resolve_includes(&main_yaml, Path::new(".gitlab-ci.yml"));
        assert!(resolved.has_unresolved_includes);
    }

    #[test]
    fn test_no_includes() {
        let main_yaml = serde_yaml::from_str::<Value>(
            r#"
stages:
  - build

build:
  stage: build
  script: echo "building"
"#,
        )
        .unwrap();

        let resolved = resolve_includes(&main_yaml, Path::new(".gitlab-ci.yml"));
        assert!(!resolved.has_unresolved_includes);

        let merged_map = resolved.merged_yaml.as_mapping().unwrap();
        assert!(merged_map.contains_key(&Value::String("build".to_string())));
    }

    #[test]
    fn test_string_include() {
        let dir = TempDir::new().unwrap();
        write_yaml(
            dir.path(),
            "child.yml",
            r#"
child-job:
  script: echo "from child"
"#,
        );

        let main_yaml = serde_yaml::from_str::<Value>(&format!(
            r#"
include: child.yml

parent-job:
  script: echo "from parent"
"#
        ))
        .unwrap();

        let resolved = resolve_includes(&main_yaml, &dir.path().join(".gitlab-ci.yml"));
        let merged_map = resolved.merged_yaml.as_mapping().unwrap();
        assert!(merged_map.contains_key(&Value::String("child-job".to_string())));
        assert!(merged_map.contains_key(&Value::String("parent-job".to_string())));
    }

    #[test]
    fn test_include_with_missing_local_file() {
        let main_yaml = serde_yaml::from_str::<Value>(&format!(
            r#"
include:
  - local: nonexistent.yml

build:
  script: echo "building"
"#
        ))
        .unwrap();

        let resolved = resolve_includes(&main_yaml, Path::new(".gitlab-ci.yml"));
        assert!(resolved.has_unresolved_includes);
        assert_eq!(resolved.failed_local_includes, vec!["nonexistent.yml"]);

        let merged_map = resolved.merged_yaml.as_mapping().unwrap();
        assert!(merged_map.contains_key(&Value::String("build".to_string())));
    }

    #[test]
    fn test_mixed_local_and_project_includes() {
        let dir = TempDir::new().unwrap();
        write_yaml(
            dir.path(),
            "local-template.yml",
            r#"
.local_template:
  script: echo "template"
"#,
        );

        let main_yaml = serde_yaml::from_str::<Value>(&format!(
            r#"
include:
  - local: local-template.yml
  - project: technology/shared
    file: shared.yml

build:
  script: echo "building"
"#
        ))
        .unwrap();

        let resolved = resolve_includes(&main_yaml, &dir.path().join(".gitlab-ci.yml"));
        let merged_map = resolved.merged_yaml.as_mapping().unwrap();

        assert!(merged_map.contains_key(&Value::String(".local_template".to_string())));
        assert!(resolved.has_unresolved_includes);
    }

    #[test]
    fn test_duplicate_key_override() {
        let dir = TempDir::new().unwrap();
        write_yaml(
            dir.path(),
            "override.yml",
            r#"
build:
  script: echo "overridden"
  stage: deploy
"#,
        );

        let main_yaml = serde_yaml::from_str::<Value>(&format!(
            r#"
stages:
  - build
  - deploy

include:
  - local: override.yml

build:
  stage: build
  script: echo "original"
"#
        ))
        .unwrap();

        let resolved = resolve_includes(&main_yaml, &dir.path().join(".gitlab-ci.yml"));
        let merged_map = resolved.merged_yaml.as_mapping().unwrap();
        let build_key = Value::String("build".to_string());
        let build = merged_map.get(&build_key).unwrap().as_mapping().unwrap();

        // Later include file overrides the main key
        assert_eq!(
            build
                .get(&Value::String("script".to_string()))
                .unwrap()
                .as_str()
                .unwrap(),
            "echo \"overridden\""
        );
    }
}
