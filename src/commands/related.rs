use crate::config::GlobalConfig;
use crate::project::Project;
use crate::transform::expand_placeholders;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RelatedError {
    #[error("No project root found (no .projections.json or matching global heuristic in parent directories)")]
    NoProjectRoot,
    #[error("Failed to load projections: {0}")]
    ConfigError(#[from] crate::config::ConfigError),
}

/// Information about a related file
#[derive(Debug, Clone)]
pub struct RelatedFile {
    /// Full path to the related file
    pub path: PathBuf,
    /// Optional search pattern for auto-scroll (from `define`)
    pub search_pattern: Option<String>,
    /// Whether the file exists
    pub exists: bool,
}

/// Result of finding related files
#[derive(Debug)]
pub struct RelatedResult {
    /// List of related files with metadata
    pub files: Vec<RelatedFile>,
    /// The project root used
    pub project_root: PathBuf,
}

/// Find related files for a given file path
pub fn find_related(
    file_path: &Path,
    global: &GlobalConfig,
) -> Result<RelatedResult, RelatedError> {
    let project = Project::discover(file_path, global)?.ok_or(RelatedError::NoProjectRoot)?;
    let matches = project.match_file(file_path);

    let mut related_files = Vec::new();
    let mut seen_paths = std::collections::HashSet::new();

    for m in matches {
        // Process related patterns
        if let Some(related_patterns) = &m.config.related {
            for pattern in related_patterns {
                let expanded = expand_placeholders(pattern, &m.placeholders);
                let full_path = project.resolve(&expanded);

                if seen_paths.insert(full_path.clone()) {
                    let search_pattern = m
                        .config
                        .define
                        .as_ref()
                        .map(|d| expand_placeholders(d, &m.placeholders));

                    related_files.push(RelatedFile {
                        exists: full_path.exists(),
                        path: full_path,
                        search_pattern,
                    });
                }
            }
        }

        // Also include alternates as related files
        for alternate_pattern in m.config.alternate.iter().flatten() {
            let expanded = expand_placeholders(alternate_pattern, &m.placeholders);
            let full_path = project.resolve(&expanded);

            if seen_paths.insert(full_path.clone()) {
                let search_pattern = m
                    .config
                    .define
                    .as_ref()
                    .map(|d| expand_placeholders(d, &m.placeholders));

                related_files.push(RelatedFile {
                    exists: full_path.exists(),
                    path: full_path,
                    search_pattern,
                });
            }
        }
    }

    Ok(RelatedResult {
        files: related_files,
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

        let config = r#"{
            "src/components/*.tsx": {
                "alternate": "src/components/{}.test.tsx",
                "related": ["src/components/{}.module.css", "src/components/{}.stories.tsx"],
                "define": "export const {}"
            }
        }"#;
        fs::write(root.join(".projections.json"), config).unwrap();

        // Create directories
        fs::create_dir_all(root.join("src/components")).unwrap();

        // Create files
        fs::write(root.join("src/components/Button.tsx"), "export const Button").unwrap();
        fs::write(root.join("src/components/Button.module.css"), ".button {}").unwrap();

        temp_dir
    }

    #[test]
    fn test_find_related() {
        let temp_dir = setup_test_project();
        let source_file = temp_dir.path().join("src/components/Button.tsx");

        let result = find_related(&source_file, &GlobalConfig::default()).unwrap();

        // Should find: Button.module.css (exists), Button.stories.tsx (not exists), Button.test.tsx (not exists)
        assert_eq!(result.files.len(), 3);

        let css_file = result
            .files
            .iter()
            .find(|f| f.path.to_string_lossy().contains("module.css"));
        assert!(css_file.is_some());
        assert!(css_file.unwrap().exists);

        // Check search pattern is expanded
        for file in &result.files {
            assert_eq!(file.search_pattern, Some("export const Button".to_string()));
        }
    }
}
