use crate::include_resolver;
use crate::reference_resolver;
use crate::schema::{SchemaType, SchemaValidator};
use crate::workflow;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use thiserror::Error;
use wrkflw_models::gitlab::Pipeline;
use wrkflw_models::ValidationResult;

/// Root-level keys that belong to Pipeline (not jobs).
/// Everything else is treated as a job via `#[serde(flatten)]`.
const PIPELINE_FIELDS: &[&str] = &[
    "image", "variables", "stages", "before_script", "after_script",
    "default", "workflow", "include",
];

#[derive(Error, Debug)]
pub enum GitlabParserError {
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("YAML parsing error: {0}")]
    YamlError(#[from] serde_yaml::Error),

    #[error("Invalid pipeline structure: {0}")]
    InvalidStructure(String),

    #[error("Schema validation error: {0}")]
    SchemaValidationError(String),
}

/// Parse a GitLab CI/CD pipeline file
pub fn parse_pipeline(pipeline_path: &Path) -> Result<Pipeline, GitlabParserError> {
    // Read the pipeline file
    let pipeline_content = fs::read_to_string(pipeline_path)?;

    // Parse YAML to serde_yaml::Value (preserves tags)
    let raw_yaml: serde_yaml::Value =
        serde_yaml::from_str(&pipeline_content).map_err(GitlabParserError::from)?;

    // Resolve local include directives before any other processing.
    // This merges jobs/templates from included files into the root document,
    // so that !reference resolution and structural validation can find them.
    let resolved = include_resolver::resolve_includes(&raw_yaml, pipeline_path);

    // Pre-scan for jobs with !reference in extends before resolution.
    // These will have their extends field replaced with a path array after
    // resolution, so the structural validator must skip the "extends undefined"
    // check for them.
    // Run this on the merged document so includes are considered.
    let tagged_extends_jobs = scan_tagged_extends(&resolved.merged_yaml);

    // Resolve !reference tags on the merged document
    // (includes may contain templates referenced by !reference tags)
    let resolved_yaml = reference_resolver::resolve_references(&resolved.merged_yaml);

    // Convert resolved YAML to JSON for schema validation
    let json_value: serde_json::Value = serde_json::to_value(&resolved_yaml).map_err(|e| {
        GitlabParserError::InvalidStructure(format!("Failed to convert YAML to JSON: {}", e))
    })?;

    // Validate against schema
    let validator = SchemaValidator::new().map_err(GitlabParserError::SchemaValidationError)?;

    validator
        .validate_json_value(&json_value, SchemaType::GitLab)
        .map_err(GitlabParserError::SchemaValidationError)?;

    // Deserialize resolved YAML into Pipeline struct
    // Filter root-level values: `#[serde(flatten)]` on jobs means every root key
    // not declared on Pipeline is tried as a Job. Non-mapping values (e.g., YAML
    // anchors like `.npm_config_setup` which is an array) would fail as Job
    // struct deserialization, so we strip them before passing to serde.
    let filtered_yaml = filter_root_for_deserialization(&resolved_yaml);
    let mut pipeline: Pipeline = serde_yaml::from_value(filtered_yaml)?;
    pipeline.has_unresolved_includes = resolved.has_unresolved_includes;
    let mut validation_result =
        validate_pipeline_structure(&pipeline, &tagged_extends_jobs, resolved.has_unresolved_includes);

    // Report failed local includes as validation issues
    for path in &resolved.failed_local_includes {
        validation_result.add_issue(format!(
            "Failed to resolve local include '{}': file not found or invalid",
            path
        ));
    }

    if !validation_result.is_valid {
        return Err(GitlabParserError::InvalidStructure(
            validation_result.issues.join("; "),
        ));
    }

    Ok(pipeline)
}

/// Scan raw YAML for jobs whose `extends` field contains a `!reference` tag.
///
/// These jobs will have their extends replaced by a path array after
/// resolution, so downstream validators must skip "extends undefined"
/// checks for them.
fn scan_tagged_extends(raw: &serde_yaml::Value) -> HashSet<String> {
    let mut result = HashSet::new();

    let root_map = match raw.as_mapping() {
        Some(m) => m,
        None => return result,
    };

    for (key, value) in root_map {
        let job_name = match key.as_str() {
            Some(s) => s,
            None => continue,
        };

        // Skip hidden jobs (template anchors)
        if job_name.starts_with('.') {
            continue;
        }

        let job_map = match value.as_mapping() {
            Some(m) => m,
            None => continue,
        };

        let extends_key = serde_yaml::Value::String("extends".to_string());
        let extends_val = match job_map.get(&extends_key) {
            Some(v) => v,
            None => continue,
        };

        if has_tagged_reference(extends_val) {
            result.insert(job_name.to_string());
        }
    }

    result
}

/// Check if a value (or any element in a sequence) is a `!reference` tag.
fn has_tagged_reference(val: &serde_yaml::Value) -> bool {
    match val {
        serde_yaml::Value::Tagged(tagged) => tagged.tag == "!reference",
        serde_yaml::Value::Sequence(seq) => seq.iter().any(has_tagged_reference),
        _ => false,
    }
}

/// Validate the basic structure of a GitLab CI/CD pipeline
///
/// `tagged_extends_jobs` — set of job names whose `extends` field contained a
/// `!reference` tag before resolution. The extends check is skipped for these
/// jobs since their extends value is a path array, not a job name.
///
/// `has_unresolved_includes` — when true, "extends undefined" and "depends on
/// undefined" checks are skipped because those jobs may be defined in includes
/// that couldn't be resolved locally (e.g., project, remote, or template
/// includes).
pub fn validate_pipeline_structure(
    pipeline: &Pipeline,
    tagged_extends_jobs: &HashSet<String>,
    has_unresolved_includes: bool,
) -> ValidationResult {
    let mut result = ValidationResult::new();

    // Check for at least one job
    if pipeline.jobs.is_empty() {
        result.add_issue("Pipeline must contain at least one job".to_string());
    }

    // Check for script in jobs
    for (job_name, job) in &pipeline.jobs {
        // Skip template and hidden jobs
        if job_name.starts_with('.') || job.template == Some(true) {
            continue;
        }

        // Check for script, extends, or trigger
        if job.script.is_none() && job.extends.is_none() && job.trigger.is_none() {
            result.add_issue(format!(
                "Job '{}' must have a script section, extend another job, or define a trigger",
                job_name
            ));
        }
    }

    // Check that referenced stages are defined
    if let Some(stages) = &pipeline.stages {
        for (job_name, job) in &pipeline.jobs {
            if job_name.starts_with('.') {
                continue;
            }
            if let Some(stage) = &job.stage {
                // .pre and .post are built-in GitLab CI stages that don't
                // need to be declared in the stages list
                if stage == ".pre" || stage == ".post" {
                    continue;
                }
                if !stages.contains(stage) {
                    result.add_issue(format!(
                        "Job '{}' references undefined stage '{}'",
                        job_name, stage
                    ));
                }
            }
        }
    }

    // Check that job dependencies exist
    // Skipped when there are unresolved includes, since the dependency may be
    // defined in an unresolvable project/remote/template include.
    if !has_unresolved_includes {
        for (job_name, job) in &pipeline.jobs {
            if job_name.starts_with('.') {
                continue;
            }
            if let Some(dependencies) = &job.dependencies {
                for dependency in dependencies {
                    if !pipeline.jobs.contains_key(dependency) {
                        result.add_issue(format!(
                            "Job '{}' depends on undefined job '{}'",
                            job_name, dependency
                        ));
                    }
                }
            }
        }
    }

    // Check that job extensions exist
    // Skip hidden jobs and jobs whose extends contained a !reference tag
    // Skipped entirely when there are unresolved includes, since the extension
    // target may be defined in an unresolvable include.
    if !has_unresolved_includes {
        for (job_name, job) in &pipeline.jobs {
            if job_name.starts_with('.') || tagged_extends_jobs.contains(job_name.as_str()) {
                continue;
            }
            if let Some(extends) = &job.extends {
                for extend in extends {
                    if !pipeline.jobs.contains_key(extend) {
                        result.add_issue(format!(
                            "Job '{}' extends undefined job '{}'",
                            job_name, extend
                        ));
                    }
                }
            }
        }
    }

    result
}

/// Convert a GitLab CI/CD pipeline to a format compatible with the workflow executor
pub fn convert_to_workflow_format(pipeline: &Pipeline) -> workflow::WorkflowDefinition {
    // Create a new workflow with required fields
    let mut workflow = workflow::WorkflowDefinition {
        name: "Converted GitLab CI Pipeline".to_string(),
        on: vec!["push".to_string()], // Default trigger
        on_raw: serde_yaml::Value::String("push".to_string()),
        jobs: HashMap::new(),
        defaults: None,
        env: HashMap::new(),
    };

    // Convert each GitLab job to a GitHub Actions job
    for (job_name, gitlab_job) in &pipeline.jobs {
        // Skip template jobs
        if let Some(true) = gitlab_job.template {
            continue;
        }

        // Create a new job
        let mut job = workflow::Job {
            runs_on: Some(vec!["ubuntu-latest".to_string()]), // Default runner
            needs: None,
            container: None,
            steps: Vec::new(),
            env: HashMap::new(),
            strategy: None,
            services: HashMap::new(),
            if_condition: None,
            outputs: None,
            permissions: None,
            uses: None,
            with: None,
            secrets: None,
            timeout_minutes: None,
            defaults: None,
        };

        // Add job-specific environment variables
        if let Some(variables) = &gitlab_job.variables {
            job.env.extend(variables.clone());
        }

        // Add global variables if they exist
        if let Some(variables) = &pipeline.variables {
            // Only add if not already defined at job level
            for (key, value) in variables {
                job.env.entry(key.clone()).or_insert_with(|| value.clone());
            }
        }

        // Convert before_script to steps if it exists
        if let Some(before_script) = &gitlab_job.before_script {
            for (i, cmd) in before_script.iter().enumerate() {
                job.steps.push(workflow::Step::with_run(
                    format!("Before script {}", i + 1),
                    cmd.clone(),
                ));
            }
        }

        // Convert main script to steps
        if let Some(script) = &gitlab_job.script {
            for (i, cmd) in script.iter().enumerate() {
                job.steps.push(workflow::Step::with_run(
                    format!("Run script line {}", i + 1),
                    cmd.clone(),
                ));
            }
        }

        // Convert after_script to steps if it exists
        if let Some(after_script) = &gitlab_job.after_script {
            for (i, cmd) in after_script.iter().enumerate() {
                let mut step =
                    workflow::Step::with_run(format!("After script {}", i + 1), cmd.clone());
                step.continue_on_error = Some(true); // After script should continue even if previous steps fail
                job.steps.push(step);
            }
        }

        // Add services if they exist
        if let Some(services) = &gitlab_job.services {
            for (i, service) in services.iter().enumerate() {
                let service_name = format!("service-{}", i);
                let service_image = match service {
                    wrkflw_models::gitlab::Service::Simple(name) => name.clone(),
                    wrkflw_models::gitlab::Service::Detailed { name, .. } => name.clone(),
                };

                let service = workflow::Service {
                    image: service_image,
                    ports: None,
                    env: HashMap::new(),
                    volumes: None,
                    options: None,
                };

                job.services.insert(service_name, service);
            }
        }

        // Add the job to the workflow
        workflow.jobs.insert(job_name.clone(), job);
    }

    workflow
}

/// Filter root-level YAML values to only keep entries that can be safely
/// deserialized as part of `Pipeline`.
///
/// Entries whose values are mappings (potential jobs) or are known Pipeline
/// fields are kept. Non-mapping, non-Pipeline-field entries (e.g., YAML anchor
/// definitions that are arrays) are removed, because they would fail
/// deserialization as `Job` via `#[serde(flatten)]`.
fn filter_root_for_deserialization(yaml: &serde_yaml::Value) -> serde_yaml::Value {
    let root_map = match yaml.as_mapping() {
        Some(m) => m,
        None => return yaml.clone(),
    };

    let known_fields: HashSet<&str> = PIPELINE_FIELDS.iter().copied().collect();

    let mut new_map = serde_yaml::Mapping::new();
    for (key, value) in root_map {
        match key.as_str() {
            Some(k) if known_fields.contains(k) || value.is_mapping() => {
                new_map.insert(key.clone(), value.clone());
            }
            Some(_) => {
                // Non-mapping, non-Pipeline-field → skip (would fail as Job)
            }
            None => {
                // Non-string key → skip
            }
        }
    }

    serde_yaml::Value::Mapping(new_map)
}

#[cfg(test)]
mod tests {
    use super::*;
    // use std::path::PathBuf; // unused
    use tempfile::NamedTempFile;

    #[test]
    fn test_parse_simple_pipeline() {
        // Create a temporary file with a simple GitLab CI/CD pipeline
        let file = NamedTempFile::new().unwrap();
        let content = r#"
stages:
  - build
  - test

build_job:
  stage: build
  script:
    - echo "Building..."
    - make build

test_job:
  stage: test
  script:
    - echo "Testing..."
    - make test
"#;
        fs::write(&file, content).unwrap();

        // Parse the pipeline
        let pipeline = parse_pipeline(file.path()).unwrap();

        // Validate basic structure
        assert_eq!(pipeline.stages.as_ref().unwrap().len(), 2);
        assert_eq!(pipeline.jobs.len(), 2);

        // Check job contents
        let build_job = pipeline.jobs.get("build_job").unwrap();
        assert_eq!(build_job.stage.as_ref().unwrap(), "build");
        assert_eq!(build_job.script.as_ref().unwrap().len(), 2);

        let test_job = pipeline.jobs.get("test_job").unwrap();
        assert_eq!(test_job.stage.as_ref().unwrap(), "test");
        assert_eq!(test_job.script.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_parse_pipeline_with_references() {
        let file = NamedTempFile::new().unwrap();
        let content = r#"
.rules:
  rules:
    - if: $CI_COMMIT_BRANCH == "main"
      when: always
    - if: $CI_PIPELINE_SOURCE == "merge_request_event"
      when: manual

build:
  stage: build
  script:
    - echo "Building..."
  rules: !reference [.rules, rules]
"#;
        fs::write(&file, content).unwrap();

        let pipeline = parse_pipeline(file.path()).unwrap();

        let build_job = pipeline.jobs.get("build").unwrap();
        let rules = build_job.rules.as_ref().unwrap();
        assert_eq!(rules.len(), 2);

        if let wrkflw_models::gitlab::Rule::Structured {
            if_,
            when,
            variables,
        } = &rules[0]
        {
            assert_eq!(if_.as_deref(), Some("$CI_COMMIT_BRANCH == \"main\""));
            assert_eq!(when.as_deref(), Some("always"));
            assert!(variables.is_none());
        } else {
            panic!("Expected Rule::Structured for rules[0]");
        }

        if let wrkflw_models::gitlab::Rule::Structured {
            if_,
            when,
            variables,
        } = &rules[1]
        {
            assert_eq!(
                if_.as_deref(),
                Some("$CI_PIPELINE_SOURCE == \"merge_request_event\"")
            );
            assert_eq!(when.as_deref(), Some("manual"));
            assert!(variables.is_none());
        } else {
            panic!("Expected Rule::Structured for rules[1]");
        }
    }

    #[test]
    fn test_parse_pipeline_with_external_references() {
        // When a !reference can't be resolved, the path array is flattened
        // into the parent sequence (matching GitLab's inline behavior).
        let file = NamedTempFile::new().unwrap();
        let content = r#"
stages:
  - build

build:
  stage: build
  script:
    - echo "Building..."
  rules:
    - !reference [.rules, except_config]
"#;
        fs::write(&file, content).unwrap();

        let pipeline = parse_pipeline(file.path()).unwrap();

        let build_job = pipeline.jobs.get("build").unwrap();
        let rules = build_job.rules.as_ref().unwrap();
        assert_eq!(rules.len(), 2);

        match &rules[0] {
            wrkflw_models::gitlab::Rule::Raw(value) => {
                assert_eq!(value.as_str().unwrap(), ".rules");
            }
            _ => panic!("Expected Rule::Raw for unresolved reference"),
        }
        match &rules[1] {
            wrkflw_models::gitlab::Rule::Raw(value) => {
                assert_eq!(value.as_str().unwrap(), "except_config");
            }
            _ => panic!("Expected Rule::Raw for unresolved reference"),
        }
    }

    #[test]
    fn test_parse_pipeline_reference_in_script() {
        let file = NamedTempFile::new().unwrap();
        let content = r#"
.scripts:
  script:
    - echo "build step 1"
    - echo "build step 2"

build:
  stage: build
  script: !reference [.scripts, script]

test:
  stage: test
  script: !reference [.nonexistent, script]
"#;
        fs::write(&file, content).unwrap();

        let pipeline = parse_pipeline(file.path()).unwrap();

        // Resolved reference: script should contain the referenced commands
        let build_job = pipeline.jobs.get("build").unwrap();
        let build_script = build_job.script.as_ref().unwrap();
        assert_eq!(build_script.len(), 2);
        assert_eq!(build_script[0], "echo \"build step 1\"");
        assert_eq!(build_script[1], "echo \"build step 2\"");

        // Unresolved reference: script should contain the path array
        let test_job = pipeline.jobs.get("test").unwrap();
        let test_script = test_job.script.as_ref().unwrap();
        assert_eq!(test_script.len(), 2);
        assert_eq!(test_script[0], ".nonexistent");
        assert_eq!(test_script[1], "script");
    }

    #[test]
    fn test_parse_pipeline_reference_in_before_after_script() {
        let file = NamedTempFile::new().unwrap();
        let content = r#"
.templates:
  before_script:
    - export VAR=hello
  after_script:
    - echo "Cleanup"

build:
  stage: build
  script:
    - echo "Building..."
  before_script: !reference [.templates, before_script]
  after_script: !reference [.templates, after_script]
"#;
        fs::write(&file, content).unwrap();

        let pipeline = parse_pipeline(file.path()).unwrap();

        let build_job = pipeline.jobs.get("build").unwrap();

        let before_script = build_job.before_script.as_ref().unwrap();
        assert_eq!(before_script.len(), 1);
        assert_eq!(before_script[0], "export VAR=hello");

        let after_script = build_job.after_script.as_ref().unwrap();
        assert_eq!(after_script.len(), 1);
        assert_eq!(after_script[0], "echo \"Cleanup\"");
    }

    #[test]
    fn test_parse_pipeline_reference_in_extends() {
        let file = NamedTempFile::new().unwrap();
        let content = r#"
.templates:
  extends:
    - .base-job
    - .default-job

stages:
  - build

build:
  stage: build
  script:
    - echo "Building..."
  extends: !reference [.templates, extends]
"#;
        fs::write(&file, content).unwrap();

        let pipeline = parse_pipeline(file.path()).unwrap();

        let build_job = pipeline.jobs.get("build").unwrap();
        let extends = build_job.extends.as_ref().unwrap();
        assert_eq!(extends.len(), 2);
        assert_eq!(extends[0], ".base-job");
        assert_eq!(extends[1], ".default-job");
    }

    #[test]
    fn test_parse_pipeline_reference_in_dependencies() {
        let file = NamedTempFile::new().unwrap();
        let content = r#"
.deps:
  dependencies:
    - build
    - test

stages:
  - build
  - test
  - deploy

build:
  stage: build
  script:
    - echo "Building..."

test:
  stage: test
  script:
    - echo "Testing..."

deploy:
  stage: deploy
  script:
    - echo "Deploying..."
  dependencies: !reference [.deps, dependencies]
"#;
        fs::write(&file, content).unwrap();

        let pipeline = parse_pipeline(file.path()).unwrap();

        let deploy_job = pipeline.jobs.get("deploy").unwrap();
        let dependencies = deploy_job.dependencies.as_ref().unwrap();
        assert_eq!(dependencies.len(), 2);
        assert_eq!(dependencies[0], "build");
        assert_eq!(dependencies[1], "test");
    }

    #[test]
    fn test_parse_pipeline_extends_reference_undeclared_target() {
        // extends: !reference to an undeclared target should NOT produce
        // a false "extends undefined job" validation error
        let file = NamedTempFile::new().unwrap();
        let content = r#"
stages:
  - build

build:
  stage: build
  script:
    - echo "Building..."
  extends: !reference [.templates, build_job]
"#;
        fs::write(&file, content).unwrap();

        // Should parse without InvalidStructure error for extends
        let pipeline = parse_pipeline(file.path()).unwrap();

        let build_job = pipeline.jobs.get("build").unwrap();
        let extends = build_job.extends.as_ref().unwrap();
        assert_eq!(extends.len(), 2);
        assert_eq!(extends[0], ".templates");
        assert_eq!(extends[1], "build_job");
    }

    #[test]
    fn test_validate_regular_extends_still_validates() {
        // Regular extends: job_name (untagged) should still fail validation
        // when the target doesn't exist
        let file = NamedTempFile::new().unwrap();
        let content = r#"
stages:
  - build

build:
  stage: build
  script:
    - echo "Building..."
  extends: nonexistent_job
"#;
        fs::write(&file, content).unwrap();

        let result = parse_pipeline(file.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("extends undefined"));
        assert!(err.contains("nonexistent_job"));
    }
}
