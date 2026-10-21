use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to read configuration file: {0}")]
    ReadError(#[from] std::io::Error),
    #[error("Failed to parse configuration: {0}")]
    ParseError(#[from] serde_json::Error),
    #[error("Failed to parse global configuration {}: {1}", .0.display())]
    GlobalParseError(PathBuf, serde_json::Error),
    #[error("No projections.json found (looked for .projections.json and .zed/projections.json)")]
    NotFound,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ProjectionConfig {
    /// Alternate file patterns, in order of preference (a string or an array in JSON)
    #[serde(default, deserialize_with = "one_or_many")]
    pub alternate: Option<Vec<String>>,
    /// Array of related file patterns
    pub related: Option<Vec<String>>,
    /// File type identifier for categorization
    #[serde(rename = "type")]
    pub file_type: Option<String>,
    /// Template lines for new file creation
    pub template: Option<Vec<String>>,
    /// Search pattern for auto-scroll in related files
    pub define: Option<String>,
    /// Additional properties (vim-projectionist compatibility)
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Deserialize a single string as a one-element list, as vim-projectionist does
fn one_or_many<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged, expecting = "a string or an array of strings")]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }

    Ok(
        Option::<OneOrMany>::deserialize(deserializer)?.map(|value| match value {
            OneOrMany::One(one) => vec![one],
            OneOrMany::Many(many) => many,
        }),
    )
}

/// Map of glob patterns to their configurations
pub type Projections = HashMap<String, ProjectionConfig>;

/// Load projections from a .projections.json file
pub fn load_projections(path: &Path) -> Result<Projections, ConfigError> {
    if !path.exists() {
        return Err(ConfigError::NotFound);
    }
    let content = fs::read_to_string(path)?;
    let projections: Projections = serde_json::from_str(&content)?;
    Ok(projections)
}

/// Configuration file paths to check, in priority order
pub const CONFIG_PATHS: &[&str] = &[".projections.json", ".zed/projections.json"];

/// Load projections from a project root directory
/// Checks for config files in priority order: .projections.json, .zed/projections.json
pub fn load_projections_from_root(project_root: &Path) -> Result<Projections, ConfigError> {
    for config_path in CONFIG_PATHS {
        let full_path = project_root.join(config_path);
        if full_path.exists() {
            return load_projections(&full_path);
        }
    }
    Err(ConfigError::NotFound)
}

/// Location of the global config: `$XDG_CONFIG_HOME/projectionist/projections.json`,
/// falling back to `~/.config/projectionist/projections.json`
pub fn global_config_path() -> Option<PathBuf> {
    let config_home = env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config_home.join("projectionist").join("projections.json"))
}

/// Projections from the user's global config, keyed by heuristic
///
/// A heuristic is a `|`-separated list of alternatives, each a `&`-separated
/// list of paths checked relative to a candidate project root. A path with a
/// trailing `/` must be a directory, any other path must be a file, and a
/// leading `!` requires the path to be absent. For example, `Gemfile&config/`
/// matches directories containing both a `Gemfile` file and a `config` directory.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(transparent)]
pub struct GlobalConfig {
    heuristics: BTreeMap<String, Projections>,
}

impl GlobalConfig {
    /// Load the global config from its default location, if it exists
    pub fn load() -> Result<Self, ConfigError> {
        match global_config_path() {
            Some(path) => Self::load_from(&path),
            None => Ok(Self::default()),
        }
    }

    /// Load a global config file, treating a missing file as empty
    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        serde_json::from_str(&content)
            .map_err(|e| ConfigError::GlobalParseError(path.to_path_buf(), e))
    }

    /// Whether any heuristic matches `dir`
    pub fn matches(&self, dir: &Path) -> bool {
        self.heuristics
            .keys()
            .any(|heuristic| heuristic_matches(heuristic, dir))
    }

    /// Projections from every heuristic that matches `dir`, in heuristic order
    pub fn projections_for(&self, dir: &Path) -> Vec<(String, ProjectionConfig)> {
        self.heuristics
            .iter()
            .filter(|(heuristic, _)| heuristic_matches(heuristic, dir))
            .flat_map(|(_, projections)| projections.clone())
            .collect()
    }
}

fn heuristic_matches(heuristic: &str, dir: &Path) -> bool {
    heuristic.split('|').any(|alternative| {
        alternative.split('&').all(|check| {
            let (negated, path) = match check.strip_prefix('!') {
                Some(path) => (true, path),
                None => (false, check),
            };
            let found = match path.strip_suffix('/') {
                Some(dir_path) => dir.join(dir_path).is_dir(),
                None => dir.join(path).is_file(),
            };
            found != negated
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn global_config(json: &str) -> GlobalConfig {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn test_load_projections() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(".projections.json");

        let config_content = r#"{
            "src/*.ts": {
                "alternate": "test/{}.test.ts",
                "type": "source"
            },
            "test/*.test.ts": {
                "alternate": "src/{}.ts",
                "type": "test"
            }
        }"#;

        let mut file = fs::File::create(&config_path).unwrap();
        file.write_all(config_content.as_bytes()).unwrap();

        let projections = load_projections_from_root(temp_dir.path()).unwrap();
        assert_eq!(projections.len(), 2);
        assert!(projections.contains_key("src/*.ts"));
        assert!(projections.contains_key("test/*.test.ts"));
    }

    #[test]
    fn test_projection_config_parsing() {
        let json = r#"{
            "alternate": "test/{}.test.ts",
            "related": ["docs/{}.md"],
            "type": "source",
            "template": ["export class {} {", "}"],
            "define": "class {}"
        }"#;

        let config: ProjectionConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.alternate, Some(vec!["test/{}.test.ts".to_string()]));
        assert_eq!(config.related, Some(vec!["docs/{}.md".to_string()]));
        assert_eq!(config.file_type, Some("source".to_string()));
        assert_eq!(
            config.template,
            Some(vec!["export class {} {".to_string(), "}".to_string()])
        );
        assert_eq!(config.define, Some("class {}".to_string()));
    }

    #[test]
    fn test_alternate_accepts_string_or_array() {
        let single: ProjectionConfig =
            serde_json::from_str(r#"{"alternate": "test/{}.test.ts"}"#).unwrap();
        assert_eq!(single.alternate, Some(vec!["test/{}.test.ts".to_string()]));

        let many: ProjectionConfig =
            serde_json::from_str(r#"{"alternate": ["a/{}.tsx", "b/{}/Index.tsx"]}"#).unwrap();
        assert_eq!(
            many.alternate,
            Some(vec!["a/{}.tsx".to_string(), "b/{}/Index.tsx".to_string()])
        );

        let none: ProjectionConfig = serde_json::from_str(r#"{"type": "source"}"#).unwrap();
        assert_eq!(none.alternate, None);

        let err = serde_json::from_str::<ProjectionConfig>(r#"{"alternate": 1}"#).unwrap_err();
        assert!(err.to_string().contains("a string or an array of strings"));
    }

    #[test]
    fn test_load_projections_from_zed_directory() {
        let temp_dir = TempDir::new().unwrap();
        let zed_dir = temp_dir.path().join(".zed");
        fs::create_dir(&zed_dir).unwrap();

        let config_content = r#"{
            "src/*.rs": {
                "alternate": "tests/{}.rs",
                "type": "source"
            }
        }"#;

        fs::write(zed_dir.join("projections.json"), config_content).unwrap();

        let projections = load_projections_from_root(temp_dir.path()).unwrap();
        assert_eq!(projections.len(), 1);
        assert!(projections.contains_key("src/*.rs"));
    }

    #[test]
    fn test_root_projections_takes_priority_over_zed() {
        let temp_dir = TempDir::new().unwrap();

        // Create .zed/projections.json first
        let zed_dir = temp_dir.path().join(".zed");
        fs::create_dir(&zed_dir).unwrap();
        fs::write(
            zed_dir.join("projections.json"),
            r#"{"zed/*.ts": {"type": "zed"}}"#,
        )
        .unwrap();

        // Create .projections.json (should take priority)
        fs::write(
            temp_dir.path().join(".projections.json"),
            r#"{"root/*.ts": {"type": "root"}}"#,
        )
        .unwrap();

        let projections = load_projections_from_root(temp_dir.path()).unwrap();
        assert_eq!(projections.len(), 1);
        assert!(projections.contains_key("root/*.ts"));
        assert!(!projections.contains_key("zed/*.ts"));
    }

    #[test]
    fn test_heuristic_matches_files_and_directories() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("Gemfile"), "").unwrap();
        fs::create_dir(dir.join("config")).unwrap();

        assert!(heuristic_matches("Gemfile", dir));
        assert!(heuristic_matches("config/", dir));
        assert!(!heuristic_matches("config", dir), "config is not a file");
        assert!(
            !heuristic_matches("Gemfile/", dir),
            "Gemfile is not a directory"
        );
        assert!(!heuristic_matches("Cargo.toml", dir));
    }

    #[test]
    fn test_heuristic_operators() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("Gemfile"), "").unwrap();

        assert!(heuristic_matches("Gemfile&!config/", dir));
        assert!(!heuristic_matches("Gemfile&config/", dir));
        assert!(heuristic_matches("Cargo.toml|Gemfile", dir));
        assert!(!heuristic_matches("Cargo.toml|go.mod", dir));
        assert!(heuristic_matches("!Cargo.toml", dir));
    }

    #[test]
    fn test_global_config_projections_for_matching_heuristics() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("Cargo.toml"), "").unwrap();

        let global = global_config(
            r#"{
                "Cargo.toml": {"src/*.rs": {"type": "rust"}},
                "Cargo.toml&src/": {"src/main.rs": {"type": "main"}},
                "go.mod": {"*.go": {"type": "go"}}
            }"#,
        );

        assert!(global.matches(temp_dir.path()));
        let globs: Vec<String> = global
            .projections_for(temp_dir.path())
            .into_iter()
            .map(|(glob, _)| glob)
            .collect();
        assert_eq!(globs, ["src/*.rs"]);

        fs::create_dir(temp_dir.path().join("src")).unwrap();
        let globs: Vec<String> = global
            .projections_for(temp_dir.path())
            .into_iter()
            .map(|(glob, _)| glob)
            .collect();
        assert_eq!(globs, ["src/*.rs", "src/main.rs"]);
    }

    #[test]
    fn test_global_config_missing_file_is_empty() {
        let temp_dir = TempDir::new().unwrap();
        let global = GlobalConfig::load_from(&temp_dir.path().join("missing.json")).unwrap();
        assert!(!global.matches(temp_dir.path()));
    }

    #[test]
    fn test_global_config_parse_error_names_the_file() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("projections.json");
        fs::write(&path, "{ not json").unwrap();

        let err = GlobalConfig::load_from(&path).unwrap_err();
        assert!(matches!(err, ConfigError::GlobalParseError(..)));
        assert!(err.to_string().contains(&path.display().to_string()));
    }
}
