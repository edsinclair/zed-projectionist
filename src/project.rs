use std::path::{Path, PathBuf};

use crate::config::{load_projections_from_root, ConfigError, GlobalConfig, CONFIG_PATHS};
use crate::matcher::{MatchResult, Matcher};

/// Project markers that indicate a project root
const PROJECT_MARKERS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "package.json",
    "Cargo.toml",
    "go.mod",
    "pyproject.toml",
    "setup.py",
    "pom.xml",
    "build.gradle",
    "Gemfile",
];

/// Check if any projections config file exists in the given directory
fn has_projections_config(dir: &Path) -> bool {
    CONFIG_PATHS.iter().any(|p| dir.join(p).exists())
}

/// Find the project root by walking up the directory tree
///
/// Priority:
/// 1. Directory containing a projections config file (highest priority)
/// 2. Directory containing a project marker (.git, package.json, etc.)
pub fn find_project_root(starting_path: &Path) -> Option<PathBuf> {
    let starting_dir = if starting_path.is_file() {
        starting_path.parent()?
    } else {
        starting_path
    };

    let mut current = starting_dir;
    let mut marker_root: Option<PathBuf> = None;

    loop {
        // Check for projections config first (highest priority)
        if has_projections_config(current) {
            return Some(current.to_path_buf());
        }

        // Check for project markers
        if marker_root.is_none() {
            for marker in PROJECT_MARKERS {
                if current.join(marker).exists() {
                    marker_root = Some(current.to_path_buf());
                    break;
                }
            }
        }

        // Move to parent directory
        match current.parent() {
            Some(parent) if parent != current => {
                current = parent;
            }
            _ => break,
        }
    }

    // If we found a marker but no projections config, use the marker root
    marker_root
}

/// Find the project root, requiring a projections config to exist
pub fn find_project_root_with_projections(starting_path: &Path) -> Option<PathBuf> {
    let starting_dir = if starting_path.is_file() {
        starting_path.parent()?
    } else {
        starting_path
    };

    let mut current = starting_dir;

    loop {
        if has_projections_config(current) {
            return Some(current.to_path_buf());
        }

        match current.parent() {
            Some(parent) if parent != current => {
                current = parent;
            }
            _ => break,
        }
    }

    None
}

/// A project root and the projections that apply within it
pub struct Project {
    pub root: PathBuf,
    pub matcher: Matcher,
}

impl Project {
    /// Find the project containing `file_path`
    ///
    /// The nearest ancestor with a projections config is the root, and global
    /// heuristics that match it add their projections after its own. Without a
    /// projections config, the nearest ancestor matching a global heuristic is
    /// the root.
    pub fn discover(file_path: &Path, global: &GlobalConfig) -> Result<Option<Self>, ConfigError> {
        let (root, local) = match find_project_root_with_projections(file_path) {
            Some(root) => {
                let local = load_projections_from_root(&root)?;
                (root, local.into_iter().collect())
            }
            None => {
                let start = if file_path.is_dir() {
                    Some(file_path)
                } else {
                    file_path.parent()
                };
                match start.and_then(|dir| dir.ancestors().find(|dir| global.matches(dir))) {
                    Some(root) => (root.to_path_buf(), Vec::new()),
                    None => return Ok(None),
                }
            }
        };

        let matcher = Matcher::layered([local, global.projections_for(&root)]);
        Ok(Some(Self { root, matcher }))
    }

    /// Match a file against this project's projections, most specific first
    pub fn match_file(&self, file_path: &Path) -> Vec<MatchResult> {
        self.matcher.match_absolute_path(file_path, &self.root)
    }

    /// Resolve a path expanded from a projection against the project root
    ///
    /// Empty placeholders can leave repeated separators, e.g. `test/{dirname}/{basename}.rs`
    /// for a top-level file expands to `test//foo.rs`. Those are collapsed.
    pub fn resolve(&self, expanded: &str) -> PathBuf {
        self.root
            .join(Path::new(expanded).components().collect::<PathBuf>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn global_config(json: &str) -> GlobalConfig {
        serde_json::from_str(json).unwrap()
    }

    fn globs(matches: &[MatchResult]) -> Vec<&str> {
        matches.iter().map(|m| m.glob.as_str()).collect()
    }

    #[test]
    fn test_find_project_root_with_projections() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();

        // Create .projections.json
        fs::write(project_root.join(".projections.json"), "{}").unwrap();

        // Create nested directory
        let nested = project_root.join("src").join("components");
        fs::create_dir_all(&nested).unwrap();

        let found = find_project_root(&nested).unwrap();
        assert_eq!(found, project_root);
    }

    #[test]
    fn test_find_project_root_with_git() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();

        // Create .git directory
        fs::create_dir(project_root.join(".git")).unwrap();

        // Create nested directory
        let nested = project_root.join("src").join("components");
        fs::create_dir_all(&nested).unwrap();

        let found = find_project_root(&nested).unwrap();
        assert_eq!(found, project_root);
    }

    #[test]
    fn test_find_project_root_projections_takes_priority() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();

        // Create both .git and .projections.json in different directories
        fs::create_dir(project_root.join(".git")).unwrap();

        let subproject = project_root.join("subproject");
        fs::create_dir(&subproject).unwrap();
        fs::write(subproject.join(".projections.json"), "{}").unwrap();

        let nested = subproject.join("src");
        fs::create_dir(&nested).unwrap();

        // Starting from nested, should find subproject (with .projections.json)
        let found = find_project_root(&nested).unwrap();
        assert_eq!(found, subproject);
    }

    #[test]
    fn test_find_project_root_with_projections_only() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();

        // Create .git but no .projections.json
        fs::create_dir(project_root.join(".git")).unwrap();

        let nested = project_root.join("src");
        fs::create_dir(&nested).unwrap();

        // Should return None when requiring projections
        let found = find_project_root_with_projections(&nested);
        assert!(found.is_none());

        // Now add .projections.json
        fs::write(project_root.join(".projections.json"), "{}").unwrap();

        let found = find_project_root_with_projections(&nested).unwrap();
        assert_eq!(found, project_root);
    }

    #[test]
    fn test_find_project_root_with_zed_projections() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();

        // Create .zed/projections.json
        fs::create_dir(project_root.join(".zed")).unwrap();
        fs::write(project_root.join(".zed/projections.json"), "{}").unwrap();

        // Create nested directory
        let nested = project_root.join("src").join("components");
        fs::create_dir_all(&nested).unwrap();

        let found = find_project_root(&nested).unwrap();
        assert_eq!(found, project_root);

        let found = find_project_root_with_projections(&nested).unwrap();
        assert_eq!(found, project_root);
    }

    #[test]
    fn test_root_projections_takes_priority_over_zed() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();

        // Create both .projections.json and .zed/projections.json
        fs::write(project_root.join(".projections.json"), r#"{"root": true}"#).unwrap();
        fs::create_dir(project_root.join(".zed")).unwrap();
        fs::write(project_root.join(".zed/projections.json"), r#"{"zed": true}"#).unwrap();

        // Both should find project root (priority doesn't matter for root finding)
        let nested = project_root.join("src");
        fs::create_dir(&nested).unwrap();

        let found = find_project_root(&nested).unwrap();
        assert_eq!(found, project_root);
    }

    #[test]
    fn test_discover_with_local_config_only() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        fs::write(root.join(".projections.json"), r#"{"src/*.rs": {}}"#).unwrap();
        fs::create_dir(root.join("src")).unwrap();
        let file = root.join("src/lib.rs");

        let project = Project::discover(&file, &GlobalConfig::default())
            .unwrap()
            .unwrap();
        assert_eq!(project.root, root);
        assert_eq!(globs(&project.match_file(&file)), ["src/*.rs"]);
    }

    #[test]
    fn test_discover_without_config_or_heuristic() {
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("src/lib.rs");

        let project = Project::discover(&file, &GlobalConfig::default()).unwrap();
        assert!(project.is_none());
    }

    #[test]
    fn test_discover_uses_nearest_heuristic_match_as_root() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("crate");
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::write(root.join("Cargo.toml"), "").unwrap();
        let file = root.join("src/nested/lib.rs");

        let global = global_config(r#"{"Cargo.toml": {"src/*.rs": {"alternate": "tests/{}.rs"}}}"#);
        let project = Project::discover(&file, &global).unwrap().unwrap();

        assert_eq!(project.root, root);
        let matches = project.match_file(&file);
        assert_eq!(globs(&matches), ["src/*.rs"]);
        assert_eq!(matches[0].placeholders.get("").unwrap(), "nested/lib");
    }

    #[test]
    fn test_discover_prefers_local_config_over_nearer_heuristic() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        fs::write(root.join(".projections.json"), r#"{"crates/*.rs": {}}"#).unwrap();
        let crate_dir = root.join("crates/foo");
        fs::create_dir_all(&crate_dir).unwrap();
        fs::write(crate_dir.join("Cargo.toml"), "").unwrap();
        let file = crate_dir.join("lib.rs");

        let global = global_config(r#"{"Cargo.toml": {"*.rs": {}}}"#);
        let project = Project::discover(&file, &global).unwrap().unwrap();

        assert_eq!(project.root, root);
        assert_eq!(globs(&project.match_file(&file)), ["crates/*.rs"]);
    }

    #[test]
    fn test_discover_puts_local_projections_before_global() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        fs::write(root.join(".projections.json"), r#"{"*.rs": {}}"#).unwrap();
        fs::write(root.join("Cargo.toml"), "").unwrap();
        let file = root.join("src/lib.rs");

        let global = global_config(r#"{"Cargo.toml": {"src/*.rs": {}}}"#);
        let project = Project::discover(&file, &global).unwrap().unwrap();

        // The local projection comes first, even though the global one is more specific
        assert_eq!(globs(&project.match_file(&file)), ["*.rs", "src/*.rs"]);
    }

    #[test]
    fn test_resolve_collapses_empty_path_segments() {
        let project = Project {
            root: PathBuf::from("/project"),
            matcher: Matcher::new(Default::default()),
        };
        assert_eq!(
            project.resolve("test//foo.rs"),
            PathBuf::from("/project/test/foo.rs")
        );
    }
}
