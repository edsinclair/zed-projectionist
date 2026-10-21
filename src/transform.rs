use regex::Regex;
use std::collections::HashMap;

/// Apply a sequence of transformations to a value
pub fn apply_transformations(value: &str, transformations: &[&str]) -> String {
    let mut result = value.to_string();

    for transform in transformations {
        result = apply_single_transformation(&result, transform);
    }

    result
}

fn apply_single_transformation(value: &str, transform: &str) -> String {
    match transform {
        // Path separator transformations
        "dot" => value.replace('/', "."),
        "underscore" => value.replace('/', "_"),
        "backslash" => value.replace('/', "\\"),
        "colons" => value.replace('/', "::"),
        "hyphenate" => {
            let re = Regex::new(r"[_\s]").unwrap();
            re.replace_all(value, "-").to_string()
        }

        // Case transformations
        "uppercase" => value.to_uppercase(),
        "lowercase" => value.to_lowercase(),
        "camelcase" => to_camel_case(value),
        "capitalize" => capitalize(value),
        "snakecase" => to_snake_case(value),

        // Path component transformations
        "dirname" => {
            if let Some(pos) = value.rfind('/') {
                value[..pos].to_string()
            } else {
                String::new()
            }
        }
        "basename" => {
            if let Some(pos) = value.rfind('/') {
                value[pos + 1..].to_string()
            } else {
                value.to_string()
            }
        }
        "file" => {
            // Get the file name without its last extension
            let basename = if let Some(pos) = value.rfind('/') {
                &value[pos + 1..]
            } else {
                value
            };
            if let Some(pos) = basename.rfind('.') {
                if pos > 0 {
                    // Handle paths: keep the directory part if present
                    if let Some(dir_pos) = value.rfind('/') {
                        format!("{}/{}", &value[..dir_pos], &basename[..pos])
                    } else {
                        basename[..pos].to_string()
                    }
                } else {
                    value.to_string()
                }
            } else {
                value.to_string()
            }
        }
        "ext" => {
            if let Some(pos) = value.rfind('.') {
                value[pos + 1..].to_string()
            } else {
                String::new()
            }
        }

        // Inflection transformations
        "plural" => pluralize(value),
        "singular" => singularize(value),

        // Literal braces, for templates that need them
        "open" => "{".to_string(),
        "close" => "}".to_string(),

        // Unknown transformation - return unchanged
        _ => value.to_string(),
    }
}

fn to_camel_case(value: &str) -> String {
    let re = Regex::new(r"[-_\s](.)?").unwrap();
    re.replace_all(value, |caps: &regex::Captures| {
        caps.get(1)
            .map(|m| m.as_str().to_uppercase())
            .unwrap_or_default()
    })
    .to_string()
}

/// Uppercase the first letter of each `/`-separated component
fn capitalize(value: &str) -> String {
    value
        .split('/')
        .map(|component| {
            let mut chars = component.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn to_snake_case(value: &str) -> String {
    let re = Regex::new(r"([a-z])([A-Z])").unwrap();
    re.replace_all(value, "${1}_${2}").to_lowercase()
}

fn pluralize(word: &str) -> String {
    // Simple English pluralization rules
    if word.ends_with("s")
        || word.ends_with("x")
        || word.ends_with("z")
        || word.ends_with("ch")
        || word.ends_with("sh")
    {
        format!("{}es", word)
    } else if word.ends_with('y') && word.len() > 1 {
        let second_to_last = word.chars().nth(word.len() - 2);
        if let Some(c) = second_to_last {
            if !"aeiou".contains(c) {
                return format!("{}ies", &word[..word.len() - 1]);
            }
        }
        format!("{}s", word)
    } else if word.ends_with("fe") {
        format!("{}ves", &word[..word.len() - 2])
    } else if word.ends_with('f') {
        format!("{}ves", &word[..word.len() - 1])
    } else {
        format!("{}s", word)
    }
}

fn singularize(word: &str) -> String {
    // Simple singularization
    if word.ends_with("ies") && word.len() > 3 {
        format!("{}y", &word[..word.len() - 3])
    } else if word.ends_with("ves") && word.len() > 3 {
        format!("{}f", &word[..word.len() - 3])
    } else if word.ends_with("ses") && word.len() > 3 {
        word[..word.len() - 2].to_string()
    } else if word.ends_with('s') && word.len() > 1 {
        word[..word.len() - 1].to_string()
    } else {
        word.to_string()
    }
}

/// Expand placeholders in a template string
///
/// Placeholder syntax:
/// - `{}` - Default placeholder (the captured stem)
/// - `{name}` - Named placeholder
/// - `{transform}` or `{|transform}` - Default placeholder with transformation
/// - `{t1|t2}` - Chained transformations (applied left to right)
/// - `{name|transform}` - Named placeholder with transformation
///
/// As in vim-projectionist, the first segment names a placeholder only if one
/// with that name exists; otherwise it is the first transformation.
pub fn expand_placeholders(template: &str, placeholders: &HashMap<String, String>) -> String {
    let re = Regex::new(r"\{([^{}]*)\}").unwrap();

    re.replace_all(template, |caps: &regex::Captures| {
        let content = caps.get(1).map(|m| m.as_str()).unwrap_or("");

        if content.is_empty() {
            // Empty braces {} use the default placeholder
            return placeholders.get("").cloned().unwrap_or_default();
        }

        let parts: Vec<&str> = content.split('|').collect();

        // A leading empty segment (`{|transform}`) selects the default placeholder
        let (value, transformations) = match placeholders.get(parts[0]) {
            Some(value) => (value.clone(), &parts[1..]),
            None => (
                placeholders.get("").cloned().unwrap_or_default(),
                &parts[..],
            ),
        };

        apply_transformations(&value, transformations)
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_transformations() {
        assert_eq!(apply_transformations("foo/bar/baz", &["dot"]), "foo.bar.baz");
        assert_eq!(
            apply_transformations("foo/bar/baz", &["underscore"]),
            "foo_bar_baz"
        );
        assert_eq!(
            apply_transformations("foo/bar/baz", &["backslash"]),
            "foo\\bar\\baz"
        );
        assert_eq!(
            apply_transformations("foo/bar/baz", &["colons"]),
            "foo::bar::baz"
        );
    }

    #[test]
    fn test_case_transformations() {
        assert_eq!(apply_transformations("fooBar", &["uppercase"]), "FOOBAR");
        assert_eq!(apply_transformations("FooBar", &["lowercase"]), "foobar");
        assert_eq!(apply_transformations("foo_bar", &["camelcase"]), "fooBar");
        assert_eq!(apply_transformations("fooBar", &["snakecase"]), "foo_bar");
    }

    #[test]
    fn test_path_component_transformations() {
        assert_eq!(
            apply_transformations("foo/bar/baz", &["dirname"]),
            "foo/bar"
        );
        assert_eq!(apply_transformations("foo/bar/baz", &["basename"]), "baz");
        assert_eq!(apply_transformations("foo.test.ts", &["file"]), "foo.test");
        assert_eq!(apply_transformations("foo.ts", &["ext"]), "ts");
    }

    #[test]
    fn test_inflection_transformations() {
        assert_eq!(apply_transformations("user", &["plural"]), "users");
        assert_eq!(apply_transformations("category", &["plural"]), "categories");
        assert_eq!(apply_transformations("leaf", &["plural"]), "leaves");
        assert_eq!(apply_transformations("bus", &["plural"]), "buses");

        assert_eq!(apply_transformations("users", &["singular"]), "user");
        assert_eq!(
            apply_transformations("categories", &["singular"]),
            "category"
        );
        assert_eq!(apply_transformations("leaves", &["singular"]), "leaf");
    }

    #[test]
    fn test_chained_transformations() {
        assert_eq!(
            apply_transformations("foo/bar", &["basename", "uppercase"]),
            "BAR"
        );
        assert_eq!(
            apply_transformations("user_profile", &["camelcase", "uppercase"]),
            "USERPROFILE"
        );
    }

    #[test]
    fn test_expand_placeholders_basic() {
        let mut placeholders = HashMap::new();
        placeholders.insert(String::new(), "utils".to_string());

        assert_eq!(expand_placeholders("{}", &placeholders), "utils");
        assert_eq!(expand_placeholders("test/{}.test.ts", &placeholders), "test/utils.test.ts");
    }

    #[test]
    fn test_expand_placeholders_with_transforms() {
        let mut placeholders = HashMap::new();
        placeholders.insert(String::new(), "foo/bar/baz".to_string());

        assert_eq!(expand_placeholders("{|dirname}", &placeholders), "foo/bar");
        assert_eq!(expand_placeholders("{|basename}", &placeholders), "baz");
        assert_eq!(expand_placeholders("{|dot}", &placeholders), "foo.bar.baz");
    }

    #[test]
    fn test_expand_placeholders_with_chained_transforms() {
        let mut placeholders = HashMap::new();
        placeholders.insert(String::new(), "foo/bar".to_string());

        assert_eq!(
            expand_placeholders("{|basename|uppercase}", &placeholders),
            "BAR"
        );
    }

    #[test]
    fn test_expand_placeholders_leading_transform() {
        let mut placeholders = HashMap::new();
        placeholders.insert(String::new(), "user_profile/admin_role".to_string());

        assert_eq!(
            expand_placeholders("{camelcase|capitalize|dot}", &placeholders),
            "UserProfile.AdminRole"
        );
        assert_eq!(
            expand_placeholders("{underscore|camelcase}", &placeholders),
            "userProfileAdminRole"
        );
        assert_eq!(
            expand_placeholders("{uppercase}", &placeholders),
            "USER_PROFILE/ADMIN_ROLE"
        );
    }

    #[test]
    fn test_expand_placeholders_named_placeholder_before_transforms() {
        let mut placeholders = HashMap::new();
        placeholders.insert(String::new(), "accounts/hash_password".to_string());
        placeholders.insert("dirname".to_string(), "accounts".to_string());
        placeholders.insert("basename".to_string(), "hash_password".to_string());

        assert_eq!(
            expand_placeholders(
                "{dirname|capitalize}.{basename|camelcase|capitalize}",
                &placeholders
            ),
            "Accounts.HashPassword"
        );
    }

    #[test]
    fn test_expand_placeholders_literal_braces() {
        let mut placeholders = HashMap::new();
        placeholders.insert(String::new(), "utils".to_string());

        assert_eq!(
            expand_placeholders(
                "import {open} test {close} from \"@/lib/{}\";",
                &placeholders
            ),
            "import { test } from \"@/lib/utils\";"
        );
        assert_eq!(expand_placeholders("{open}{close}", &placeholders), "{}");
    }

    #[test]
    fn test_capitalize() {
        assert_eq!(apply_transformations("foo", &["capitalize"]), "Foo");
        assert_eq!(
            apply_transformations("foo/bar_baz", &["capitalize"]),
            "Foo/Bar_baz"
        );
        assert_eq!(apply_transformations("", &["capitalize"]), "");
    }

    #[test]
    fn test_hyphenate() {
        assert_eq!(apply_transformations("foo_bar", &["hyphenate"]), "foo-bar");
        assert_eq!(
            apply_transformations("foo bar", &["hyphenate"]),
            "foo-bar"
        );
    }
}
