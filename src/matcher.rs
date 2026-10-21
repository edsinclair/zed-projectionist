use crate::config::{ProjectionConfig, Projections};
use crate::transform::apply_transformations;
use std::cmp::{Ordering, Reverse};
use std::collections::HashMap;
use std::path::Path;

/// Result of matching a file against projections
#[derive(Debug, Clone)]
pub struct MatchResult {
    /// The glob pattern that matched
    pub glob: String,
    /// The configuration for this pattern
    pub config: ProjectionConfig,
    /// Extracted placeholders ('' key contains the stem, plus 'dirname' and 'basename')
    pub placeholders: HashMap<String, String>,
}

/// A parsed projection glob
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pattern {
    /// No `*`: matches the path exactly, with an empty stem
    Exact(String),
    /// `prefix*suffix`: the stem is everything in between, slashes included
    Star { prefix: String, suffix: String },
    /// `prefix**infix*suffix`, e.g. `src/**/*.ts` or `tests/**/test_*.py`
    ///
    /// The infix is removed from the start of the last path component, so
    /// `tests/foo/test_bar.py` matched against `tests/**/test_*.py` has the
    /// stem `foo/bar`.
    Recursive {
        prefix: String,
        infix: String,
        suffix: String,
    },
}

impl Pattern {
    /// Parse a glob, returning `None` for shapes that can never match
    fn parse(glob: &str) -> Option<Self> {
        match glob.matches('*').count() {
            0 => Some(Self::Exact(glob.to_string())),
            1 => {
                let (prefix, suffix) = glob.split_once('*')?;
                Some(Self::Star {
                    prefix: prefix.to_string(),
                    suffix: suffix.to_string(),
                })
            }
            3 => {
                let (prefix, rest) = glob.split_once("**")?;
                let (infix, suffix) = rest.split_once('*')?;
                (!prefix.contains('*') && !infix.is_empty()).then(|| Self::Recursive {
                    prefix: prefix.to_string(),
                    infix: infix.to_string(),
                    suffix: suffix.to_string(),
                })
            }
            _ => None,
        }
    }

    /// Return the stem captured from `path`, if it matches
    fn capture(&self, path: &str) -> Option<String> {
        match self {
            Self::Exact(exact) => (path == exact).then(String::new),
            Self::Star { prefix, suffix } => path
                .strip_prefix(prefix.as_str())?
                .strip_suffix(suffix.as_str())
                .map(str::to_string),
            Self::Recursive {
                prefix,
                infix,
                suffix,
            } => {
                let matched = path
                    .strip_prefix(prefix.as_str())?
                    .strip_suffix(suffix.as_str())?;

                // Find the infix where no `/` follows it, i.e. at the start of
                // the last component. The leading `/` lets `**/` match zero directories.
                let slashed = format!("/{}", matched);
                let start = (0..slashed.len()).find(|&i| {
                    slashed
                        .get(i..)
                        .and_then(|tail| tail.strip_prefix(infix.as_str()))
                        .is_some_and(|rest| !rest.contains('/'))
                })?;

                let stem = format!("{}/{}", &slashed[..start], &slashed[start + infix.len()..]);
                Some(stem[1..].to_string())
            }
        }
    }
}

/// Order globs from most to least specific: exact paths first, then longer
/// globs before shorter ones, with ties broken alphabetically
fn by_specificity(a: &str, b: &str) -> Ordering {
    let key = |glob: &str| (glob.contains('*'), Reverse(glob.len()));
    key(a).cmp(&key(b)).then_with(|| a.cmp(b))
}

/// Placeholders available to a match: the stem plus its dirname and basename
fn placeholders_for(stem: String) -> HashMap<String, String> {
    HashMap::from([
        (
            "dirname".to_string(),
            apply_transformations(&stem, &["dirname"]),
        ),
        (
            "basename".to_string(),
            apply_transformations(&stem, &["basename"]),
        ),
        (String::new(), stem),
    ])
}

struct Projection {
    glob: String,
    pattern: Pattern,
    config: ProjectionConfig,
}

/// Handles matching files against projection patterns
pub struct Matcher {
    /// Projections in the order matches are reported
    projections: Vec<Projection>,
}

impl Matcher {
    pub fn new(projections: Projections) -> Self {
        Self::layered([projections])
    }

    /// Build a matcher from layers of projections
    ///
    /// Every projection in a layer is reported before those in later layers.
    /// Within a layer, more specific patterns come first.
    pub fn layered<L>(layers: impl IntoIterator<Item = L>) -> Self
    where
        L: IntoIterator<Item = (String, ProjectionConfig)>,
    {
        let mut projections = Vec::new();

        for layer in layers {
            let mut layer: Vec<Projection> = layer
                .into_iter()
                .filter_map(|(glob, config)| {
                    Some(Projection {
                        pattern: Pattern::parse(&glob)?,
                        glob,
                        config,
                    })
                })
                .collect();
            layer.sort_by(|a, b| by_specificity(&a.glob, &b.glob));
            projections.extend(layer);
        }

        Self { projections }
    }

    /// Match a file path against all projections and return matching results
    ///
    /// The file path should be relative to the project root. Results are
    /// ordered most specific first.
    pub fn match_file(&self, relative_path: &str) -> Vec<MatchResult> {
        // Normalize path separators to forward slashes
        let normalized_path = relative_path.replace('\\', "/");

        self.projections
            .iter()
            .filter_map(|projection| {
                let stem = projection.pattern.capture(&normalized_path)?;
                Some(MatchResult {
                    glob: projection.glob.clone(),
                    config: projection.config.clone(),
                    placeholders: placeholders_for(stem),
                })
            })
            .collect()
    }

    /// Match an absolute file path using the project root
    pub fn match_absolute_path(&self, file_path: &Path, project_root: &Path) -> Vec<MatchResult> {
        if let Ok(relative) = file_path.strip_prefix(project_root) {
            let relative_str = relative.to_string_lossy().replace('\\', "/");
            self.match_file(&relative_str)
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProjectionConfig;

    fn create_test_projections() -> Projections {
        let mut projections = Projections::new();

        projections.insert(
            "src/*.ts".to_string(),
            ProjectionConfig {
                alternate: Some(vec!["test/{}.test.ts".to_string()]),
                file_type: Some("source".to_string()),
                ..Default::default()
            },
        );

        projections.insert(
            "test/*.test.ts".to_string(),
            ProjectionConfig {
                alternate: Some(vec!["src/{}.ts".to_string()]),
                file_type: Some("test".to_string()),
                ..Default::default()
            },
        );

        projections.insert(
            "app/models/*.rb".to_string(),
            ProjectionConfig {
                alternate: Some(vec!["spec/models/{}_spec.rb".to_string()]),
                file_type: Some("model".to_string()),
                ..Default::default()
            },
        );

        projections
    }

    fn capture(glob: &str, path: &str) -> Option<String> {
        Pattern::parse(glob).unwrap().capture(path)
    }

    fn globs(matches: &[MatchResult]) -> Vec<&str> {
        matches.iter().map(|m| m.glob.as_str()).collect()
    }

    #[test]
    fn test_star_pattern() {
        assert_eq!(
            capture("src/*.ts", "src/utils.ts").as_deref(),
            Some("utils")
        );
        assert_eq!(
            capture("src/*.ts", "src/foo/bar.ts").as_deref(),
            Some("foo/bar")
        );
        assert_eq!(capture("src/*.ts", "test/utils.ts"), None);
        assert_eq!(capture("src/*.ts", "src/utils.tsx"), None);
    }

    #[test]
    fn test_star_pattern_prefix_and_suffix_cannot_overlap() {
        assert_eq!(capture("a*a.ts", "a.ts"), None);
    }

    #[test]
    fn test_exact_pattern() {
        assert_eq!(capture("README.md", "README.md").as_deref(), Some(""));
        assert_eq!(capture("README.md", "docs/README.md"), None);
    }

    #[test]
    fn test_recursive_pattern() {
        assert_eq!(
            capture("src/**/*.ts", "src/utils.ts").as_deref(),
            Some("utils")
        );
        assert_eq!(
            capture("src/**/*.ts", "src/foo/bar.ts").as_deref(),
            Some("foo/bar")
        );
        assert_eq!(capture("src/**/*.ts", "lib/foo/bar.ts"), None);
    }

    #[test]
    fn test_recursive_pattern_with_infix() {
        let glob = "tests/**/test_*.py";
        assert_eq!(capture(glob, "tests/test_bar.py").as_deref(), Some("bar"));
        assert_eq!(
            capture(glob, "tests/foo/test_bar.py").as_deref(),
            Some("foo/bar")
        );
        assert_eq!(
            capture(glob, "tests/test_foo/test_bar.py").as_deref(),
            Some("test_foo/bar")
        );
        assert_eq!(capture(glob, "tests/foo/bar.py"), None);
        assert_eq!(capture(glob, "tests/test_foo/bar.py"), None);
    }

    #[test]
    fn test_unsupported_patterns_are_skipped() {
        assert_eq!(Pattern::parse("src/*/*.ts"), None);
        assert_eq!(Pattern::parse("src/***.ts"), None);
        assert_eq!(Pattern::parse("*/**/*.ts"), None);

        let mut projections = Projections::new();
        projections.insert("src/*/*.ts".to_string(), ProjectionConfig::default());
        assert!(Matcher::new(projections)
            .match_file("src/a/b.ts")
            .is_empty());
    }

    #[test]
    fn test_match_file() {
        let projections = create_test_projections();
        let matcher = Matcher::new(projections);

        let matches = matcher.match_file("src/utils.ts");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].glob, "src/*.ts");
        assert_eq!(matches[0].placeholders.get("").unwrap(), "utils");
    }

    #[test]
    fn test_match_file_nested_path() {
        let projections = create_test_projections();
        let matcher = Matcher::new(projections);

        let matches = matcher.match_file("src/components/Button.ts");
        assert_eq!(matches.len(), 1);
        assert_eq!(
            matches[0].placeholders.get("").unwrap(),
            "components/Button"
        );
    }

    #[test]
    fn test_match_file_test_pattern() {
        let projections = create_test_projections();
        let matcher = Matcher::new(projections);

        let matches = matcher.match_file("test/utils.test.ts");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].glob, "test/*.test.ts");
        assert_eq!(matches[0].placeholders.get("").unwrap(), "utils");
    }

    #[test]
    fn test_match_file_no_match() {
        let projections = create_test_projections();
        let matcher = Matcher::new(projections);

        let matches = matcher.match_file("other/file.js");
        assert!(matches.is_empty());
    }

    #[test]
    fn test_match_file_ruby_model() {
        let projections = create_test_projections();
        let matcher = Matcher::new(projections);

        let matches = matcher.match_file("app/models/user.rb");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].glob, "app/models/*.rb");
        assert_eq!(matches[0].placeholders.get("").unwrap(), "user");
    }

    #[test]
    fn test_match_file_dirname_and_basename_placeholders() {
        let mut projections = Projections::new();
        projections.insert("src/**/*.rs".to_string(), ProjectionConfig::default());
        let matcher = Matcher::new(projections);

        let nested = &matcher.match_file("src/foo/bar.rs")[0].placeholders;
        assert_eq!(nested.get("dirname").unwrap(), "foo");
        assert_eq!(nested.get("basename").unwrap(), "bar");

        let top_level = &matcher.match_file("src/bar.rs")[0].placeholders;
        assert_eq!(top_level.get("dirname").unwrap(), "");
        assert_eq!(top_level.get("basename").unwrap(), "bar");
    }

    #[test]
    fn test_match_file_orders_most_specific_first() {
        // HashMap iteration order varies between instances, so build it a few times
        for _ in 0..20 {
            let projections: Projections =
                ["*.go", "*_test.go", "pkg/*_test.go", "pkg/foo_test.go"]
                    .into_iter()
                    .map(|glob| (glob.to_string(), ProjectionConfig::default()))
                    .collect();
            let matcher = Matcher::new(projections);

            assert_eq!(
                globs(&matcher.match_file("pkg/foo_test.go")),
                ["pkg/foo_test.go", "pkg/*_test.go", "*_test.go", "*.go"]
            );
        }
    }

    #[test]
    fn test_match_file_breaks_ties_alphabetically() {
        let projections: Projections = ["a/*.ts", "*.ts", "*/b.ts"]
            .into_iter()
            .map(|glob| (glob.to_string(), ProjectionConfig::default()))
            .collect();
        let matcher = Matcher::new(projections);

        assert_eq!(
            globs(&matcher.match_file("a/b.ts")),
            ["*/b.ts", "a/*.ts", "*.ts"]
        );
    }

    #[test]
    fn test_layered_matcher_reports_earlier_layers_first() {
        let config = ProjectionConfig::default;
        let local = vec![("*.rs".to_string(), config())];
        let global = vec![
            ("src/*.rs".to_string(), config()),
            ("*.rs".to_string(), config()),
        ];
        let matcher = Matcher::layered([local, global]);

        assert_eq!(
            globs(&matcher.match_file("src/lib.rs")),
            ["*.rs", "src/*.rs", "*.rs"]
        );
    }
}
