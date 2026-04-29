use clap::{Parser, Subcommand};
use glyim_cli::pipeline::{self, BuildMode};
use glyim_pkg::manifest::{Dependency, PackageManifest};
use std::path::PathBuf;
use std::process;

#[derive(Parser)]
#[command(
    name = "glyim",
    version,
    about = "The Glyim compiler",
    after_help = "Examples:\n  glyim init myproject\n  glyim run src/main.g\n  glyim check src/main.g\n  glyim ir src/main.g"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Build {
        input: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long, conflicts_with = "release")]
        debug: bool,
        #[arg(long, conflicts_with = "debug")]
        release: bool,
    },
    Run {
        input: PathBuf,
        #[arg(long, conflicts_with = "release")]
        debug: bool,
        #[arg(long, conflicts_with = "debug")]
        release: bool,
    },
    Ir {
        input: PathBuf,
    },
    Check {
        input: PathBuf,
    },
    Init {
        name: String,
    },
    Export {
        name: String,
        dest: PathBuf,
    },
    Test {
        input: PathBuf,
        #[arg(long)]
        ignore: bool,
        #[arg(long)]
        filter: Option<String>,
    },
    Add {
        package: String,
        #[arg(long)]
        macro_dep: bool,
    },
    Remove {
        package: String,
    },
    Fetch,
    Publish {
        #[arg(long)]
        dry_run: bool,
    },
    Outdated,
}

fn main() {
    let cli = Cli::parse();
    let exit_code = match cli.command {
        Command::Build {
            input,
            output,
            debug: _,
            release,
        } => {
            let mode = if release {
                BuildMode::Release
            } else {
                BuildMode::Debug
            };
            let result = if input.is_dir() {
                pipeline::build_package(&input, output.as_deref(), mode)
            } else {
                pipeline::build_with_mode(&input, output.as_deref(), mode)
            };
            match result {
                Ok(path) => {
                    eprintln!("Built: {}", path.display());
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        Command::Run {
            input,
            debug: _,
            release,
        } => {
            let mode = if release {
                BuildMode::Release
            } else {
                BuildMode::Debug
            };
            let result = if input.is_dir() {
                pipeline::run_package(&input, mode)
            } else {
                pipeline::run_with_mode(&input, mode)
            };
            match result {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        Command::Ir { input } => match pipeline::print_ir(&input) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("error: {e}");
                1
            }
        },
        Command::Check { input } => match pipeline::check(&input) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("error: {e}");
                1
            }
        },
        Command::Init { name } => match pipeline::init(&name) {
            Ok(path) => {
                eprintln!("Created {}/", path.display());
                0
            }
            Err(e) => {
                eprintln!("error: {e}");
                1
            }
        },
        Command::Test {
            input,
            ignore,
            filter,
        } => {
            let include_ignored = ignore;
            let result = if input.is_dir() {
                pipeline::run_tests_package(&input, filter.as_deref(), include_ignored)
            } else {
                pipeline::run_tests(&input, filter.as_deref(), include_ignored)
            };
            match result {
                Ok(summary) => {
                    eprintln!("{}", summary.format_summary());
                    summary.exit_code()
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        Command::Export { name, dest } => {
            eprintln!(
                "error: 'export' not implemented (artifact: {name}, dest: {})",
                dest.display()
            );
            1
        }
        Command::Add { package, macro_dep } => {
            let result: Result<i32, i32> = (|| {
                let dir = std::env::current_dir().map_err(|e| {
                    eprintln!("error: {e}");
                    1
                })?;
                let manifest_path = dir.join("glyim.toml");
                let toml_str = std::fs::read_to_string(&manifest_path).map_err(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        eprintln!("error: glyim.toml not found");
                    } else {
                        eprintln!("error: {e}");
                    }
                    1
                })?;
                let mut m: PackageManifest = glyim_pkg::manifest::parse_manifest(
                    &toml_str,
                    &manifest_path.to_string_lossy(),
                )
                .map_err(|e| {
                    eprintln!("error: invalid glyim.toml: {e}");
                    1
                })?;
                let target_deps = if macro_dep { &mut m.macros } else { &mut m.dependencies };
                target_deps.insert(
                    package.clone(),
                    Dependency {
                        version: Some("*".into()),
                        path: None,
                        registry: None,
                        workspace: false,
                        is_macro: macro_dep,
                    },
                );
                let new_toml = toml::to_string_pretty(&m).unwrap_or_default();
                std::fs::write(&manifest_path, new_toml).map_err(|e| {
                    eprintln!("error writing manifest: {e}");
                    1
                })?;
                eprintln!(
                    "Added {package} to {}",
                    if macro_dep { "[macros]" } else { "[dependencies]" }
                );
                Ok(0)
            })();
            result.unwrap_or_else(|code| code)
        }
        Command::Remove { package } => {
            let result: Result<i32, i32> = (|| {
                let dir = std::env::current_dir().map_err(|e| {
                    eprintln!("error: {e}");
                    1
                })?;
                let manifest_path = dir.join("glyim.toml");
                let toml_str = std::fs::read_to_string(&manifest_path).map_err(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        eprintln!("error: glyim.toml not found");
                    } else {
                        eprintln!("error: {e}");
                    }
                    1
                })?;
                let mut m: PackageManifest = glyim_pkg::manifest::parse_manifest(
                    &toml_str,
                    &manifest_path.to_string_lossy(),
                )
                .map_err(|e| {
                    eprintln!("error: invalid glyim.toml: {e}");
                    1
                })?;
                m.dependencies.remove(&package);
                m.macros.remove(&package);
                let new_toml = toml::to_string_pretty(&m).unwrap_or_default();
                std::fs::write(&manifest_path, new_toml).map_err(|e| {
                    eprintln!("error writing manifest: {e}");
                    1
                })?;
                eprintln!("Removed {package} from dependencies");
                Ok(0)
            })();
            result.unwrap_or_else(|code| code)
        }
        Command::Fetch => {
            eprintln!("Dependencies resolved (local path deps only for now)");
            0
        }
        Command::Publish { dry_run: _ } => {
            eprintln!("error: publish not yet implemented");
            1
        }
        Command::Outdated => {
            eprintln!("error: outdated not yet implemented");
            1
        }
    };
    process::exit(exit_code);
}
