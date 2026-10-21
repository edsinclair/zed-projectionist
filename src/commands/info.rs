use crate::config::GlobalConfig;
use crate::matcher::MatchResult;
use crate::project::Project;
use crate::transform::expand_placeholders;
use serde::Serialize;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum InfoError {
    #[error("No project root found (no .projections.json or matching global heuristic in parent directories)")]
    NoProjectRoot,
    #[error("Failed to load projections: {0}")]
    ConfigError(#[from] crate::config::ConfigError),
}

/// Detailed information about a file's projections
#[derive(Debug, Serialize)]
pub struct ProjectionInfo {
    /// The glob pattern that matched
    pub pattern: String,
    /// The captured stem value
    pub stem: String,
    /// The file type (if specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_type: Option<String>,
    /// Expanded alternate file paths
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub alternate: Vec<ExpandedPath>,
    /// Expanded related file paths
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<ExpandedPath>,
    /// Define pattern (search pattern for related files)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub define: Option<String>,
    /// Template (if specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<Vec<String>>,
}

/// A path that has been expanded from a pattern
#[derive(Debug, Serialize)]
pub struct ExpandedPath {
    /// The original pattern
    pub pattern: String,
    /// The expanded path (relative)
    pub expanded: String,
    /// Full absolute path
    pub full_path: String,
    /// Whether the file exists
    pub exists: bool,
}

/// Full result of getting projection info
#[derive(Debug, Serialize)]
pub struct InfoResult {
    /// The file path that was queried
    pub file: String,
    /// The project root
    pub project_root: String,
    /// All matching projections with details
    pub projections: Vec<ProjectionInfo>,
}

/// Get detailed projection information for a file
pub fn get_projection_info(
    file_path: &Path,
    global: &GlobalConfig,
) -> Result<InfoResult, InfoError> {
    let project = Project::discover(file_path, global)?.ok_or(InfoError::NoProjectRoot)?;

    let projection_infos: Vec<ProjectionInfo> = project
        .match_file(file_path)
        .into_iter()
        .map(|m| build_projection_info(m, &project))
        .collect();

    Ok(InfoResult {
        file: file_path.to_string_lossy().to_string(),
        project_root: project.root.to_string_lossy().to_string(),
        projections: projection_infos,
    })
}

fn build_projection_info(m: MatchResult, project: &Project) -> ProjectionInfo {
    let stem = m.placeholders.get("").cloned().unwrap_or_default();

    let expand_paths = |patterns: &Option<Vec<String>>| -> Vec<ExpandedPath> {
        patterns
            .iter()
            .flatten()
            .map(|pattern| {
                let expanded = expand_placeholders(pattern, &m.placeholders);
                let full_path = project.resolve(&expanded);
                ExpandedPath {
                    pattern: pattern.clone(),
                    expanded,
                    exists: full_path.exists(),
                    full_path: full_path.to_string_lossy().to_string(),
                }
            })
            .collect()
    };

    let alternate = expand_paths(&m.config.alternate);
    let related = expand_paths(&m.config.related);

    let define = m
        .config
        .define
        .as_ref()
        .map(|d| expand_placeholders(d, &m.placeholders));

    let template = m.config.template.as_ref().map(|lines| {
        lines
            .iter()
            .map(|line| expand_placeholders(line, &m.placeholders))
            .collect()
    });

    ProjectionInfo {
        pattern: m.glob,
        stem,
        file_type: m.config.file_type,
        alternate,
        related,
        define,
        template,
    }
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
            "src/*.ts": {
                "alternate": "test/{}.test.ts",
                "related": ["docs/{}.md"],
                "type": "source",
                "define": "export class {}",
                "template": ["export class {} {", "}"]
            }
        }"#;
        fs::write(root.join(".projections.json"), config).unwrap();
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src/utils.ts"), "").unwrap();

        temp_dir
    }

    #[test]
    fn test_get_projection_info() {
        let temp_dir = setup_test_project();
        let source_file = temp_dir.path().join("src/utils.ts");

        let result = get_projection_info(&source_file, &GlobalConfig::default()).unwrap();

        assert_eq!(result.projections.len(), 1);

        let info = &result.projections[0];
        assert_eq!(info.pattern, "src/*.ts");
        assert_eq!(info.stem, "utils");
        assert_eq!(info.file_type, Some("source".to_string()));
        assert_eq!(info.define, Some("export class utils".to_string()));

        assert_eq!(info.alternate.len(), 1);
        let alt = &info.alternate[0];
        assert_eq!(alt.pattern, "test/{}.test.ts");
        assert_eq!(alt.expanded, "test/utils.test.ts");
        assert!(!alt.exists);
    }
}
