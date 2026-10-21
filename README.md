# Projectionist

Project-aware file navigation for Zed (and other editors) via `projections.json`.

A Rust port of Tim Pope's [vim-projectionist](https://github.com/tpope/vim-projectionist).

## Features

- **Alternate file navigation**: Switch between source and test files
- **Related file discovery**: Find all files related to the current file
- **Template-based file creation**: Create new files with predefined templates
- **18 transformations**: Transform placeholders (camelCase, snake_case, plural, etc.)
- **Global projections**: Apply projections to every project matching a heuristic (e.g. every Cargo project)
- **Any editor**: `--open` uses Zed by default, or `--editor` / `$PROJECTIONIST_EDITOR`

## Installation

```bash
# From source
cargo install --path .

# From crates.io (when published)
cargo install projectionist
```

## Quick Start

1. Create a `projections.json` configuration file in your project root:

   - `.projections.json` (vim-projectionist compatible)
   - `.zed/projections.json` (Zed-specific location)

   Example:

```json
{
  "src/*.ts": {
    "alternate": "test/{}.test.ts",
    "type": "source"
  },
  "test/*.test.ts": {
    "alternate": "src/{}.ts",
    "type": "test"
  }
}
```

2. Use the CLI:

```bash
# Find alternate file
projectionist alternate src/utils.ts

# Open alternate in Zed
projectionist alternate src/utils.ts --open

# Find related files
projectionist related src/components/Button.tsx

# Create file from template
projectionist create src/components/NewComponent.tsx

# Show projection info
projectionist info src/utils.ts
```

## Zed Integration

### Tasks (~/.config/zed/tasks.json)

```json
[
  {
    "label": "Switch to Alternate File",
    "command": "projectionist",
    "args": ["alternate", "$ZED_FILE", "--open"],
    "cwd": "$ZED_WORKTREE_ROOT",
    "reveal": "never",
    "hide": "always"
  },
  {
    "label": "Show Projection Info",
    "command": "projectionist",
    "args": ["info", "$ZED_FILE"],
    "cwd": "$ZED_WORKTREE_ROOT",
    "reveal": "always"
  }
]
```

### Keybindings (~/.config/zed/keymap.json)

> **Important**: You MUST include `"reevaluate_context": true` or Zed will cache the file path.

```json
[
  {
    "context": "Editor && !menu",
    "bindings": {
      "ctrl-shift-a": [
        "task::Spawn",
        { "task_name": "Switch to Alternate File", "reevaluate_context": true }
      ]
    }
  }
]
```

## Configuration File Location

The CLI searches for configuration files in the following order (first found wins):

| Location | Description |
|----------|-------------|
| `.projections.json` | Root of project (vim-projectionist compatible) |
| `.zed/projections.json` | Inside `.zed` directory (Zed-specific) |

If both files exist, `.projections.json` takes priority for vim-projectionist compatibility.

The project root is the nearest parent directory containing one of these files.

### Global configuration

Projections you want in every project of a kind can go in a global config at
`$XDG_CONFIG_HOME/projectionist/projections.json` (default `~/.config/projectionist/projections.json`).
It maps *heuristics* to projections, like vim-projectionist's `g:projectionist_heuristics`:

```json
{
  "Cargo.toml": {
    "src/*.rs": { "alternate": "tests/{}.rs", "type": "source" }
  },
  "Gemfile&config/": {
    "app/models/*.rb": { "alternate": "spec/models/{}_spec.rb", "type": "model" }
  }
}
```

A heuristic lists paths that must exist relative to the project root:

| Syntax | Meaning |
|--------|---------|
| `Cargo.toml` | A file named `Cargo.toml` exists |
| `config/` | A directory named `config` exists (trailing `/`) |
| `!config/` | No directory named `config` exists |
| `a&b` | Both `a` and `b` match |
| `a\|b` | Either `a` or `b` matches |

If a project has its own projections file, global projections whose heuristic matches that root are added
after the project's own. Without a projections file, the nearest parent directory matching a heuristic
becomes the project root.

## Configuration Format

The configuration file maps glob patterns to configurations:

```json
{
  "src/components/*.tsx": {
    "alternate": "src/components/{}.test.tsx",
    "related": ["src/components/{}.module.css", "src/components/{}.stories.tsx"],
    "type": "component",
    "template": [
      "import React from 'react';",
      "",
      "export const {}: React.FC = () => {",
      "  return <div>{}</div>;",
      "};"
    ],
    "define": "export const {}"
  }
}
```

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `alternate` | `string \| string[]` | Alternate file pattern(s), in order of preference |
| `related` | `string[]` | Array of related file patterns |
| `type` | `string` | File type identifier |
| `template` | `string[]` | Template lines for new file creation |
| `define` | `string` | Search pattern for auto-scroll |

### Patterns

| Pattern | Matches | Stem (`{}`) |
|---------|---------|-------------|
| `src/*.ts` | `src/a/b.ts` | `a/b` (everything between prefix and suffix) |
| `src/**/*.ts` | `src/b.ts`, `src/a/b.ts` | `b`, `a/b` |
| `tests/**/test_*.py` | `tests/a/test_b.py` | `a/b` (the `test_` prefix is dropped from the file name) |
| `README.md` | `README.md` only | empty |

Other patterns with more than one `*` never match.

When several projections match a file, the most specific comes first: exact paths, then longer patterns
before shorter ones, with ties broken alphabetically. So for `foo_test.go`, `*_test.go` is used before
`*.go`. This decides which alternate is used, which template `create` uses, and the order of
`related` and `info` output.

### Placeholders

| Syntax | Description |
|--------|-------------|
| `{}` | The captured stem |
| `{dirname}` | Directory portion of the stem (empty for a top-level file) |
| `{basename}` | File name portion of the stem |
| `{transform}` or `{\|transform}` | Stem with transformation |
| `{t1\|t2}` | Chained transformations, applied left to right |
| `{dirname\|transform}` | Placeholder with transformation |

As in vim-projectionist, the first name in braces is a placeholder only if one with that name exists, so
`{camelcase|capitalize|dot}` applies all three transformations to the stem (`foo_bar/baz` → `FooBar.Baz`).

For example, `"src/**/*.py": {"alternate": "tests/{dirname}/test_{basename}.py"}` maps `src/a/b.py` to
`tests/a/test_b.py` and `src/b.py` to `tests/test_b.py`.

### Transformations

**Path separators:**
- `dot` - Replace `/` with `.`
- `underscore` - Replace `/` with `_`
- `backslash` - Replace `/` with `\`
- `colons` - Replace `/` with `::`

**Case:**
- `uppercase` - Convert to UPPERCASE
- `lowercase` - Convert to lowercase
- `camelcase` - Convert to camelCase
- `capitalize` - Uppercase the first letter of each path component
- `snakecase` - Convert to snake_case
- `hyphenate` - Replace `_` with `-`

**Path components:**
- `dirname` - Directory portion
- `basename` - File name only
- `file` - Name without extension
- `ext` - Extension only

**Inflection:**
- `plural` - Pluralize (user → users)
- `singular` - Singularize (users → user)

**Literals:**
- `open` - A literal `{` (e.g. `import {open} foo {close} from "bar";`)
- `close` - A literal `}`

## Example Configurations

### React/TypeScript

```json
{
  "src/components/*.tsx": {
    "alternate": "src/components/{}.test.tsx",
    "related": ["src/components/{}.module.css"],
    "type": "component"
  },
  "src/components/*.test.tsx": {
    "alternate": "src/components/{}.tsx",
    "type": "test"
  }
}
```

### Go

```json
{
  "*.go": {
    "alternate": "{}_test.go",
    "type": "source"
  },
  "*_test.go": {
    "alternate": "{}.go",
    "type": "test"
  }
}
```

### Rails

```json
{
  "app/models/*.rb": {
    "alternate": "spec/models/{}_spec.rb",
    "related": ["db/schema.rb"],
    "define": "create_table \"{|plural}\"",
    "type": "model"
  },
  "app/controllers/*_controller.rb": {
    "alternate": "spec/controllers/{}_controller_spec.rb",
    "type": "controller"
  }
}
```

## CLI Reference

```
projectionist <COMMAND>

Commands:
  alternate  Find alternate file(s) for the given file
  related    Find related files for the given file
  create     Create a new file from template
  info       Show projection info for a file
  validate   Validate .projections.json configuration

Options:
  -h, --help     Print help
  -V, --version  Print version
```

### alternate

```
projectionist alternate <FILE> [OPTIONS]

Options:
  -l, --line <LINE>        Line number for cursor-aware features
  -o, --open               Open result in the editor (see --editor)
      --editor <EDITOR>    Editor command used by --open
                           [env: PROJECTIONIST_EDITOR] [default: zed]
  -j, --json               Output as JSON
      --create-if-missing  Create the alternate file from template if missing
  -v, --verbose            Show verbose matching information
```

### related

```
projectionist related <FILE> [OPTIONS]

Options:
  -l, --line <LINE>  Line number for cursor-aware features
  -j, --json         Output as JSON
      --fzf          Output one file per line for fzf/pipe usage
  -v, --verbose      Show verbose information
```

### create

```
projectionist create <FILE> [OPTIONS]

Options:
  -f, --force  Overwrite if file already exists
  -o, --open             Open the created file in the editor (see --editor)
      --editor <EDITOR>  Editor command used by --open
                         [env: PROJECTIONIST_EDITOR] [default: zed]
```

### info

```
projectionist info <FILE> [OPTIONS]

Options:
  -l, --line <LINE>  Line number for cursor-aware features
  -j, --json         Output as JSON
  -v, --verbose      Show verbose information
```

## License

MIT
