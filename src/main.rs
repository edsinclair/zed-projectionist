use clap::{Parser, Subcommand};
use projectionist::{commands, config, GlobalConfig};
use std::path::{Path, PathBuf};
use std::process;

#[derive(Parser)]
#[command(
    name = "projectionist",
    about = "Project-aware file navigation via .projections.json",
    version,
    author
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Find alternate file(s) for the given file
    Alternate {
        /// File path to find alternates for
        file: PathBuf,

        /// Line number for cursor-aware features
        #[arg(short, long)]
        line: Option<usize>,

        /// Open result in the editor (see --editor)
        #[arg(short, long)]
        open: bool,

        /// Editor command used by --open
        #[arg(long, env = "PROJECTIONIST_EDITOR", default_value = "zed")]
        editor: String,

        /// Output as JSON
        #[arg(short, long)]
        json: bool,

        /// Create the alternate file from template if it doesn't exist
        #[arg(long)]
        create_if_missing: bool,

        /// Show verbose matching information
        #[arg(short, long)]
        verbose: bool,
    },

    /// Find related files for the given file
    Related {
        /// File path to find related files for
        file: PathBuf,

        /// Line number for cursor-aware features
        #[arg(short, long)]
        line: Option<usize>,

        /// Output as JSON
        #[arg(short, long)]
        json: bool,

        /// Output one file per line for fzf/pipe usage
        #[arg(long)]
        fzf: bool,

        /// Show verbose information
        #[arg(short, long)]
        verbose: bool,
    },

    /// Create a new file from template
    Create {
        /// File path to create
        file: PathBuf,

        /// Overwrite if file already exists
        #[arg(short, long)]
        force: bool,

        /// Open the created file in the editor (see --editor)
        #[arg(short, long)]
        open: bool,

        /// Editor command used by --open
        #[arg(long, env = "PROJECTIONIST_EDITOR", default_value = "zed")]
        editor: String,
    },

    /// Show projection info for a file
    Info {
        /// File path to show info for
        file: PathBuf,

        /// Line number for cursor-aware features
        #[arg(short, long)]
        line: Option<usize>,

        /// Output as JSON
        #[arg(short, long)]
        json: bool,

        /// Show verbose information
        #[arg(short, long)]
        verbose: bool,
    },

    /// Validate .projections.json configuration
    Validate {
        /// Project root directory (default: auto-detect from current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{}", e);
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Alternate {
            file,
            line: _line,
            open,
            editor,
            json,
            create_if_missing,
            verbose,
        } => {
            let file = canonicalize_path(&file)?;
            let global = GlobalConfig::load()?;
            let result = commands::find_alternate(&file, &global)?;

            if verbose {
                eprintln!("Project root: {}", result.project_root.display());
                eprintln!(
                    "Potential alternates: {}",
                    result
                        .paths
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }

            if json {
                let output = serde_json::json!({
                    "alternates": result.paths.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(),
                    "existing": result.existing.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(),
                    "project_root": result.project_root.to_string_lossy()
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else if let Some(alternate) = result.existing.first() {
                if open {
                    open_in_editor(&editor, alternate)?;
                } else {
                    println!("{}", alternate.display());
                }
            } else if create_if_missing {
                if let Some(alternate) = result.paths.first() {
                    // Try to create the file
                    match commands::create_file(alternate, false, &global) {
                        Ok(created) => {
                            if verbose {
                                eprintln!("Created: {}", created.path.display());
                            }
                            if open {
                                open_in_editor(&editor, &created.path)?;
                            } else {
                                println!("{}", created.path.display());
                            }
                        }
                        Err(e) => {
                            return Err(format!(
                                "Failed to create alternate file {}: {}",
                                alternate.display(),
                                e
                            )
                            .into());
                        }
                    }
                } else {
                    return Err("No alternate file pattern found".into());
                }
            } else if !result.paths.is_empty() {
                // Alternate pattern exists but file doesn't
                if verbose {
                    eprintln!(
                        "Alternate file does not exist: {}",
                        result.paths[0].display()
                    );
                }
                println!("{}", result.paths[0].display());
            } else {
                return Err(format!("No alternate file found for: {}", file.display()).into());
            }
        }

        Commands::Related {
            file,
            line: _line,
            json,
            fzf,
            verbose,
        } => {
            let file = canonicalize_path(&file)?;
            let global = GlobalConfig::load()?;
            let result = commands::find_related(&file, &global)?;

            if verbose {
                eprintln!("Project root: {}", result.project_root.display());
                eprintln!("Found {} related files", result.files.len());
            }

            if json {
                let output: Vec<serde_json::Value> = result
                    .files
                    .iter()
                    .map(|f| {
                        serde_json::json!({
                            "path": f.path.to_string_lossy(),
                            "exists": f.exists,
                            "search_pattern": f.search_pattern
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else if fzf {
                // Output only existing files, one per line
                for f in result.files.iter().filter(|f| f.exists) {
                    println!("{}", f.path.display());
                }
            } else {
                for f in &result.files {
                    let status = if f.exists { "✓" } else { "✗" };
                    println!("{} {}", status, f.path.display());
                    if let Some(pattern) = &f.search_pattern {
                        println!("  search: {}", pattern);
                    }
                }
            }
        }

        Commands::Create {
            file,
            force,
            open,
            editor,
        } => {
            let file = if file.is_absolute() {
                file
            } else {
                std::env::current_dir()?.join(file)
            };

            let global = GlobalConfig::load()?;
            let result = commands::create_file(&file, force, &global)?;

            println!("Created: {}", result.path.display());

            if open {
                open_in_editor(&editor, &result.path)?;
            }
        }

        Commands::Info {
            file,
            line: _line,
            json,
            verbose,
        } => {
            let file = canonicalize_path(&file)?;
            let global = GlobalConfig::load()?;
            let result = commands::get_projection_info(&file, &global)?;

            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("File: {}", result.file);
                println!("Project root: {}", result.project_root);
                println!();

                if result.projections.is_empty() {
                    println!("No matching projections found.");
                } else {
                    for (i, p) in result.projections.iter().enumerate() {
                        if i > 0 {
                            println!();
                        }
                        println!("Pattern: {}", p.pattern);
                        println!("  Stem: {}", p.stem);
                        if let Some(ft) = &p.file_type {
                            println!("  Type: {}", ft);
                        }
                        for alt in &p.alternate {
                            let status = if alt.exists { "✓" } else { "✗" };
                            println!("  Alternate: {} {}", status, alt.expanded);
                        }
                        if !p.related.is_empty() {
                            println!("  Related:");
                            for r in &p.related {
                                let status = if r.exists { "✓" } else { "✗" };
                                println!("    {} {}", status, r.expanded);
                            }
                        }
                        if let Some(define) = &p.define {
                            println!("  Define: {}", define);
                        }
                        if verbose {
                            if let Some(template) = &p.template {
                                println!("  Template:");
                                for line in template {
                                    println!("    {}", line);
                                }
                            }
                        }
                    }
                }
            }
        }

        Commands::Validate { path } => {
            let path = canonicalize_path(&path)?;
            let config_path = if path.is_dir() {
                path.join(".projections.json")
            } else {
                path
            };

            match config::load_projections(&config_path) {
                Ok(projections) => {
                    println!("✓ Valid .projections.json");
                    println!("  {} projection pattern(s) defined", projections.len());
                    for (pattern, config) in &projections {
                        print!("  - {}", pattern);
                        if let Some(ft) = &config.file_type {
                            print!(" ({})", ft);
                        }
                        println!();
                    }
                }
                Err(e) => {
                    return Err(format!("✗ Invalid configuration: {}", e).into());
                }
            }
        }
    }

    Ok(())
}

fn canonicalize_path(path: &PathBuf) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if path.exists() {
        Ok(path.canonicalize()?)
    } else {
        // For non-existent files, resolve the parent and append the filename
        let parent = path.parent().unwrap_or(path);
        let filename = path.file_name();

        if parent.exists() {
            let canonical_parent = parent.canonicalize()?;
            if let Some(name) = filename {
                Ok(canonical_parent.join(name))
            } else {
                Ok(canonical_parent)
            }
        } else {
            // Just use the path as-is if we can't canonicalize
            Ok(path.clone())
        }
    }
}

/// Open `path` with `editor`, a command that may include arguments (e.g. "code --wait")
fn open_in_editor(editor: &str, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut args = editor.split_whitespace();
    let program = args.next().ok_or("Editor command is empty")?;

    let status = process::Command::new(program)
        .args(args)
        .arg(path)
        .status()
        .map_err(|e| format!("Failed to run editor `{}`: {}", program, e))?;

    if !status.success() {
        return Err(format!("Failed to open {} in {}", path.display(), program).into());
    }

    Ok(())
}
