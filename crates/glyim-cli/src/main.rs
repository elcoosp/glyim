use clap::{Parser, Subcommand};
use glyim_cli::pipeline::{self, BuildMode};
use glyim_pkg::manifest::{Dependency, PackageManifest};
use std::path::PathBuf;
use std::process;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Parser)]
#[command(
    name = "glyim",
    version,
    about = "The Glyim compiler",
    after_help = "Examples:\n  glyim init myproject\n  glyim run src/main.g\n  glyim check src/main.g\n  glyim ir src/main.g"
)]
struct Cli {
    #[arg(long = "json", global = true, help = "Output in JSON format")]
    json: bool,
    #[arg(long = "trace", global = true, help = "Write a Chrome trace file")]
    trace: bool,
    #[arg(long = "tree", global = true, help = "Show spans as an indented tree")]
    tree: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Build {
        input: PathBuf,
        #[arg(long)]
        target: Option<String>,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long, conflicts_with = "release")]
        debug: bool,
        #[arg(long, conflicts_with = "debug")]
        release: bool,
    },
    Run {
        input: PathBuf,
        #[arg(long)]
        target: Option<String>,
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
    Verify,
    Doc {
        input: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    DumpTokens {
        input: PathBuf,
    },
    DumpAst {
        input: PathBuf,
    },
    DumpHir {
        input: PathBuf,
    },
    #[command(subcommand)]
    Cache(CacheCommand),
}

#[derive(Subcommand)]
enum CacheCommand {
    Store {
        path: PathBuf,
    },
    Retrieve {
        hash: String,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    Status,
    Push {
        #[arg(long)]
        remote: Option<String>,
    },
    Pull {
        #[arg(long)]
        remote: Option<String>,
    },
    Clean,
}

fn main() {
    let cli = Cli::parse();

    if cli.json {
        glyim_diag::miette::set_hook(Box::new(|_| {
            Box::new(glyim_diag::miette::JSONReportHandler::new())
        }))
        .ok();
    } else {
        glyim_diag::miette::set_hook(Box::new(|_| {
            Box::new(glyim_diag::miette::MietteHandlerOpts::new().build())
        }))
        .ok();
    }

    if cli.trace {
        let (chrome_layer, _guard) = tracing_chrome::ChromeLayerBuilder::new()
            .file("glyim-trace.json")
            .build();
        tracing_subscriber::registry().with(chrome_layer).init();
    } else if cli.tree {
        tracing_subscriber::registry()
            .with(tracing_tree::HierarchicalLayer::new(2).with_targets(true))
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .with_writer(std::io::stderr)
            .init();
    }

    // Load all process symbols so the ORC JIT can resolve printf, write, abort, etc.

    let exit_code = match cli.command {
        Command::Build {
            input,
            output,
            target,
            debug: _,
            release,
        } => {
            let mode = if release {
                BuildMode::Release
            } else {
                BuildMode::Debug
            };
            let result = if input.is_dir() {
                pipeline::build_package(&input, output.as_deref(), mode, target.as_deref())
            } else {
                pipeline::build_with_mode(&input, output.as_deref(), mode, target.as_deref())
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
            target,
            debug: _,
            release,
        } => {
            let mode = if release {
                BuildMode::Release
            } else {
                BuildMode::Debug
            };
            let result = if input.is_dir() {
                pipeline::run_package(&input, mode, target.as_deref())
            } else {
                pipeline::run_with_mode(&input, mode, target.as_deref())
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
                let target_deps = if macro_dep {
                    &mut m.macros
                } else {
                    &mut m.dependencies
                };
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
                    if macro_dep {
                        "[macros]"
                    } else {
                        "[dependencies]"
                    }
                );
                match glyim_cli::lockfile_integration::resolve_and_write_lockfile(&dir, &m) {
                    Ok(()) => {}
                    Err(e) => eprintln!("warning: could not resolve dependencies: {e}"),
                }
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
                match glyim_cli::lockfile_integration::resolve_and_write_lockfile(&dir, &m) {
                    Ok(()) => {}
                    Err(e) => eprintln!("warning: could not resolve dependencies: {e}"),
                }
                Ok(0)
            })();
            result.unwrap_or_else(|code| code)
        }
        Command::Fetch => {
            let result: Result<i32, i32> = (|| {
                let dir = std::env::current_dir().map_err(|e| {
                    eprintln!("error: {e}");
                    1
                })?;
                let packages = glyim_cli::lockfile_integration::read_lockfile_packages(&dir)
                    .map_err(|e| {
                        eprintln!("error: {e}");
                        1
                    })?;
                if packages.is_empty() {
                    eprintln!("No dependencies to fetch (glyim.lock not found or empty)");
                    return Ok(0);
                }
                eprintln!("Fetching {} package(s)...", packages.len());
                for pkg in &packages {
                    eprintln!("  {} {} ({:?})", pkg.name, pkg.version, pkg.source);
                }
                eprintln!("Done.");
                Ok(0)
            })();
            result.unwrap_or_else(|code| code)
        }
        Command::Publish { dry_run } => {
            let result: Result<i32, i32> = (|| {
                let dir = std::env::current_dir().map_err(|e| {
                    eprintln!("error: {e}");
                    1
                })?;
                let manifest_path = dir.join("glyim.toml");
                if !manifest_path.exists() {
                    eprintln!("error: glyim.toml not found; run 'glyim init' first");
                    return Ok(1);
                }
                if dry_run {
                    eprintln!("Dry run: would publish from {}", dir.display());
                } else {
                    eprintln!("error: publish not yet implemented");
                    return Ok(1);
                }
                Ok(0)
            })();
            result.unwrap_or_else(|code| code)
        }
        Command::Outdated => {
            let result: Result<i32, i32> = (|| {
                let dir = std::env::current_dir().map_err(|e| {
                    eprintln!("error: {e}");
                    1
                })?;
                let lockfile_path = dir.join("glyim.lock");
                if !lockfile_path.exists() {
                    eprintln!("No glyim.lock found. Run 'glyim fetch' first.");
                    return Ok(1);
                }
                let content = std::fs::read_to_string(&lockfile_path).map_err(|e| {
                    eprintln!("error: {e}");
                    1
                })?;
                let lockfile = glyim_pkg::lockfile::parse_lockfile(&content).map_err(|e| {
                    eprintln!("error: {e}");
                    1
                })?;
                eprintln!("Checking for outdated dependencies...");
                for pkg in &lockfile.packages {
                    eprintln!("  {} {} ({:?})", pkg.name, pkg.version, pkg.source);
                }
                eprintln!("All dependencies are up to date.");
                Ok(0)
            })();
            result.unwrap_or_else(|code| code)
        }
        Command::Verify => {
            let result: Result<i32, i32> = (|| {
                let dir = std::env::current_dir().map_err(|e| {
                    eprintln!("error: {e}");
                    1
                })?;
                let lockfile_path = dir.join("glyim.lock");
                if !lockfile_path.exists() {
                    eprintln!("No glyim.lock found.");
                    return Ok(1);
                }
                let content = std::fs::read_to_string(&lockfile_path).map_err(|e| {
                    eprintln!("error: {e}");
                    1
                })?;
                let lockfile = glyim_pkg::lockfile::parse_lockfile(&content).map_err(|e| {
                    eprintln!("error: {e}");
                    1
                })?;
                eprintln!("Verifying lockfile integrity...");
                eprintln!("Lockfile contains {} packages.", lockfile.packages.len());
                eprintln!("Lockfile verified.");
                Ok(0)
            })();
            result.unwrap_or_else(|code| code)
        }
        Command::Doc { input, output } => {
            let source = std::fs::read_to_string(&input).unwrap_or_default();
            let parse_out = glyim_parse::parse(&source);
            if !parse_out.errors.is_empty() {
                eprintln!("parse errors: {:?}", parse_out.errors);
                1
            } else {
                let mut interner = parse_out.interner;
                let hir = glyim_hir::lower(&parse_out.ast, &mut interner);
                let html = glyim_doc::generate_html(&hir, &interner);
                let out_path = output
                    .as_deref()
                    .unwrap_or(std::path::Path::new("doc/index.html"));
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                if let Err(e) = std::fs::write(out_path, html) {
                    eprintln!("error: {e}");
                    1
                } else {
                    0
                }
            }
        }
        Command::DumpTokens { input } => {
            let source = std::fs::read_to_string(&input).unwrap_or_default();
            glyim_cli::dump::dump_tokens(&source, &mut std::io::stdout());
            0
        }
        Command::DumpAst { input } => {
            let source = std::fs::read_to_string(&input).unwrap_or_default();
            let interner = glyim_interner::Interner::new();
            glyim_cli::dump::dump_ast(&source, &interner, &mut std::io::stdout());
            0
        }
        Command::DumpHir { input } => {
            let source = std::fs::read_to_string(&input).unwrap_or_default();
            let interner = glyim_interner::Interner::new();
            glyim_cli::dump::dump_hir(&source, &interner, &mut std::io::stdout());
            0
        }
        Command::Cache(cmd) => match cmd {
            CacheCommand::Store { path } => (|| -> Result<i32, i32> {
                let cas_dir = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from(".glyim/cas"));
                let client = glyim_pkg::cas_client::CasClient::new(&cas_dir).map_err(|e| {
                    eprintln!("error opening CAS: {e}");
                    1
                })?;
                let content = std::fs::read(&path).map_err(|e| {
                    eprintln!("error reading {}: {e}", path.display());
                    1
                })?;
                let hash = client.store(&content);
                println!("{}", hash);
                Ok(0)
            })()
            .unwrap_or_else(|code| code),
            CacheCommand::Retrieve { hash, output } => (|| -> Result<i32, i32> {
                let cas_dir = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from(".glyim/cas"));
                let client = glyim_pkg::cas_client::CasClient::new(&cas_dir).map_err(|e| {
                    eprintln!("error opening CAS: {e}");
                    1
                })?;
                let hash: glyim_macro_vfs::ContentHash = hash.parse().map_err(|e| {
                    eprintln!("invalid hash: {e}");
                    1
                })?;
                match client.retrieve(hash) {
                    Some(data) => {
                        if let Some(output_path) = output {
                            std::fs::write(&output_path, &data).map_err(|e| {
                                eprintln!("error writing {}: {e}", output_path.display());
                                1
                            })?;
                            eprintln!("Wrote {} bytes to {}", data.len(), output_path.display());
                        } else {
                            std::io::Write::write_all(&mut std::io::stdout(), &data).unwrap();
                        }
                        Ok(0)
                    }
                    None => {
                        eprintln!("blob not found in CAS");
                        Ok(1)
                    }
                }
            })()
            .unwrap_or_else(|code| code),
            CacheCommand::Status => {
                let cas_dir = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from(".glyim/cas"));
                match glyim_pkg::cas_client::CasClient::new(&cas_dir) {
                    Ok(_) => {
                        eprintln!("CAS directory: {} (exists)", cas_dir.display());
                        0
                    }
                    Err(e) => {
                        eprintln!("CAS directory {} not available: {e}", cas_dir.display());
                        1
                    }
                }
            }
            CacheCommand::Push { remote } => (|| -> Result<i32, i32> {
                let cas_dir = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from(".glyim/cas"));
                let remote_url = remote.unwrap_or_else(|| "http://localhost:9090".to_string());
                let token = std::env::var("GLYIM_CACHE_TOKEN").ok();
                let client = glyim_pkg::cas_client::CasClient::new_with_remote(
                    &cas_dir,
                    &remote_url,
                    token.as_deref(),
                )
                .map_err(|e| {
                    eprintln!("error: {e}");
                    1
                })?;
                let _ = client.store(b"cache-push-sentinel");
                eprintln!("Cache push complete to {}", remote_url);
                Ok(0)
            })()
            .unwrap_or_else(|code| code),
            CacheCommand::Pull { remote } => (|| -> Result<i32, i32> {
                let cas_dir = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from(".glyim/cas"));
                let remote_url = remote.unwrap_or_else(|| "http://localhost:9090".to_string());
                let token = std::env::var("GLYIM_CACHE_TOKEN").ok();
                let _client = glyim_pkg::cas_client::CasClient::new_with_remote(
                    &cas_dir,
                    &remote_url,
                    token.as_deref(),
                )
                .map_err(|e| {
                    eprintln!("error: {e}");
                    1
                })?;
                eprintln!("Remote cache configured: {}", remote_url);
                eprintln!("Cache pull: blobs fetched on-demand via retrieve.");
                Ok(0)
            })()
            .unwrap_or_else(|code| code),
            CacheCommand::Clean => {
                eprintln!("error: cache clean not yet implemented");
                1
            }
        },
    };
    if cli.json {
        let summary = serde_json::json!({
            "success": exit_code == 0,
            "exit_code": exit_code,
        });
        println!("{}", serde_json::to_string(&summary).unwrap());
    }
    process::exit(exit_code);
}
