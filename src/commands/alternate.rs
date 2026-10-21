use crate::config::GlobalConfig;
use crate::project::Project;
use crate::transform::expand_placeholders;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AlternateError {
    #[error("No project root found (no .projections.json or matching global heuristic in parent directories)")]
    NoProjectRoot,
    #[error("Failed to load projections: {0}")]
    ConfigError(#[from] crate::config::ConfigError),
}

/// Result of finding alternate files
#[derive(Debug)]
pub struct AlternateResult {
    /// List of potential alternate file paths (absolute)
    pub paths: Vec<PathBuf>,
    /// Which paths actually exist on the filesystem
    pub existing: Vec<PathBuf>,
    /// The project root used
    pub project_root: PathBuf,
}

/// Find alternate files for a given file path
///
/// Alternates are ordered by the specificity of the projection they come from.
pub fn find_alternate(
    file_path: &Path,
    global: &GlobalConfig,
) -> Result<AlternateResult, AlternateError> {
    let project = Project::discover(file_path, global)?.ok_or(AlternateError::NoProjectRoot)?;
    let matches = project.match_file(file_path);

    let mut all_paths = Vec::new();
    let mut existing_paths = Vec::new();

    for m in matches {
        for alternate_pattern in m.config.alternate.iter().flatten() {
            let expanded = expand_placeholders(alternate_pattern, &m.placeholders);
            let full_path = project.resolve(&expanded);

            if !all_paths.contains(&full_path) {
                all_paths.push(full_path.clone());

                if full_path.exists() {
                    existing_paths.push(full_path);
                }
            }
        }
    }

    Ok(AlternateResult {
        paths: all_paths,
        existing: existing_paths,
        project_root: project.root,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_project() -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // Create .projections.json
        let config = r#"{
            "src/*.ts": {
                "alternate": "test/{}.test.ts"
            },
            "test/*.test.ts": {
                "alternate": "src/{}.ts"
            }
        }"#;
        fs::write(root.join(".projections.json"), config).unwrap();

        // Create directories
        fs::create_dir(root.join("src")).unwrap();
        fs::create_dir(root.join("test")).unwrap();

        // Create source file
        fs::write(root.join("src/utils.ts"), "// source").unwrap();

        // Create test file
        fs::write(root.join("test/utils.test.ts"), "// test").unwrap();

        temp_dir
    }

    #[test]
    fn test_find_alternate_source_to_test() {
        let temp_dir = setup_test_project();
        let source_file = temp_dir.path().join("src/utils.ts");

        let result = find_alternate(&source_file, &GlobalConfig::default()).unwrap();

        assert_eq!(result.paths.len(), 1);
        assert_eq!(result.existing.len(), 1);
        assert!(result.paths[0].ends_with("test/utils.test.ts"));
    }

    #[test]
    fn test_find_alternate_test_to_source() {
        let temp_dir = setup_test_project();
        let test_file = temp_dir.path().join("test/utils.test.ts");

        let result = find_alternate(&test_file, &GlobalConfig::default()).unwrap();

        assert_eq!(result.paths.len(), 1);
        assert_eq!(result.existing.len(), 1);
        assert!(result.paths[0].ends_with("src/utils.ts"));
    }

    #[test]
    fn test_find_alternate_nonexistent() {
        let temp_dir = setup_test_project();
        let source_file = temp_dir.path().join("src/newfile.ts");
        fs::write(&source_file, "// new file").unwrap();

        let result = find_alternate(&source_file, &GlobalConfig::default()).unwrap();

        // Path should be computed but not exist
        assert_eq!(result.paths.len(), 1);
        assert_eq!(result.existing.len(), 0);
        assert!(result.paths[0].ends_with("test/newfile.test.ts"));
    }

    #[test]
    fn test_find_alternate_recursive_glob_with_dirname_and_basename() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        let config = r#"{
            "src/**/*.py": {"alternate": "tests/{dirname}/test_{basename}.py"},
            "tests/**/test_*.py": {"alternate": "src/{}.py"}
        }"#;
        fs::write(root.join(".projections.json"), config).unwrap();
        let global = GlobalConfig::default();

        let nested = find_alternate(&root.join("src/foo/bar.py"), &global).unwrap();
        assert_eq!(nested.paths, [root.join("tests/foo/test_bar.py")]);

        let top_level = find_alternate(&root.join("src/bar.py"), &global).unwrap();
        assert_eq!(top_level.paths, [root.join("tests/test_bar.py")]);

        let test = find_alternate(&root.join("tests/foo/test_bar.py"), &global).unwrap();
        assert_eq!(test.paths, [root.join("src/foo/bar.py")]);
    }

    #[test]
    fn test_find_alternate_prefers_most_specific_projection() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        let config = r#"{
            "*.go": {"alternate": "{}_test.go"},
            "*_test.go": {"alternate": "{}.go"}
        }"#;
        fs::write(root.join(".projections.json"), config).unwrap();

        let result = find_alternate(&root.join("foo_test.go"), &GlobalConfig::default()).unwrap();
        assert_eq!(
            result.paths,
            [root.join("foo.go"), root.join("foo_test_test.go")]
        );
    }

    #[test]
    fn test_find_alternate_from_list() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        let config = r#"{
            "test/e2e/*_test.exs": {
                "alternate": ["app/pages/{camelcase|capitalize}.tsx", "app/pages/{camelcase|capitalize}/Index.tsx"]
            }
        }"#;
        fs::write(root.join(".projections.json"), config).unwrap();
        fs::create_dir_all(root.join("app/pages/Orgs")).unwrap();
        fs::write(root.join("app/pages/Orgs/Index.tsx"), "").unwrap();

        let result = find_alternate(
            &root.join("test/e2e/orgs_test.exs"),
            &GlobalConfig::default(),
        )
        .unwrap();
        assert_eq!(
            result.paths,
            [
                root.join("app/pages/Orgs.tsx"),
                root.join("app/pages/Orgs/Index.tsx")
            ]
        );
        assert_eq!(result.existing, [root.join("app/pages/Orgs/Index.tsx")]);
    }

    #[test]
    fn test_find_alternate_from_global_heuristic() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        fs::write(root.join("Cargo.toml"), "").unwrap();
        let global: GlobalConfig =
            serde_json::from_str(r#"{"Cargo.toml": {"src/*.rs": {"alternate": "tests/{}.rs"}}}"#)
                .unwrap();

        let result = find_alternate(&root.join("src/parser.rs"), &global).unwrap();
        assert_eq!(result.project_root, root);
        assert_eq!(result.paths, [root.join("tests/parser.rs")]);
    }
}
