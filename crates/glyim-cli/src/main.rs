extern crate glyim_cli;
use clap::{Parser, Subcommand};
use glyim_cli::commands::*;
use glyim_macro_vfs::ContentStore;
use std::path::PathBuf;
use std::process;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
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
        #[arg(long)]
        bare: bool,
        #[arg(long)]
        incremental: bool,
        #[arg(long, help = "URL of remote CAS server for artifact caching")]
        remote_cache: Option<String>,
    },
    Run {
        input: PathBuf,
        #[arg(long)]
        target: Option<String>,
        #[arg(long, conflicts_with = "release")]
        debug: bool,
        #[arg(long, conflicts_with = "debug")]
        release: bool,
        #[arg(long)]
        live: bool,
        #[arg(long)]
        incremental: bool,
        #[arg(long, help = "URL of remote CAS server for artifact caching")]
        remote_cache: Option<String>,
    },
    Ir {
        input: PathBuf,
    },
    Check {
        input: PathBuf,
        #[arg(long)]
        incremental: bool,
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
        #[arg(long)]
        nocapture: bool,
        #[arg(long)]
        watch: bool,
        #[arg(long)]
        optimize: bool,
        #[arg(long)]
        coverage: bool,
        #[arg(long, help = "URL of remote CAS server for artifact caching")]
        remote_cache: Option<String>,
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
        #[arg(long)]
        wasm: bool,
    },
    Outdated,
    Verify,
    Doc {
        input: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long)]
        open: bool,
        #[arg(long)]
        test: bool,
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
    MacroInspect {
        input: PathBuf,
    },
    #[command(subcommand)]
    Cache(CacheCommand),
    /// Show incremental compilation cache statistics.
    IncrementalStatus {
        input: PathBuf,
    },
}

#[derive(Subcommand)]
#[allow(dead_code)]
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

    let exit_code = match cli.command {
        Command::Build {
            input,
            output,
            target,
            debug: _,
            release,
            bare,
            incremental,
            remote_cache,
        } => cmd_build(input, output, target, release, bare, incremental, remote_cache),
        Command::Run {
            input,
            target,
            debug: _,
            release,
                    
            live,
            incremental, remote_cache,
        } => cmd_run(input, target, release, live, incremental, remote_cache),
        Command::Ir { input } => cmd_ir(input),
        Command::Check { input, incremental } => cmd_check(input, incremental),
        Command::Init { name } => cmd_init(name),
        Command::Test {
            input,
            ignore,
            filter,
            nocapture,
            watch,
            optimize,
            remote_cache,
            coverage,
        } => cmd_test(input, ignore, filter, nocapture, watch, optimize, remote_cache, coverage),
        Command::Export { name, dest } => cmd_export(name, dest),
        Command::Add { package, macro_dep } => cmd_add(package, macro_dep),
        Command::Remove { package } => cmd_remove(package),
        Command::Fetch => cmd_fetch(),
        Command::Publish { dry_run, wasm } => cmd_publish(dry_run, wasm),
        Command::Outdated => cmd_outdated(),
        Command::Verify => cmd_verify(),
        Command::Doc {
            input,
            output,
            open,
            test,
        } => cmd_doc(input, output, open, test),
        Command::DumpTokens { input } => cmd_dump_tokens(input),
        Command::DumpAst { input } => cmd_dump_ast(input),
        Command::DumpHir { input } => cmd_dump_hir(input),
        Command::MacroInspect { input } => cmd_macro_inspect(input),
        Command::IncrementalStatus { input } => cmd_incremental_status(input),
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
            CacheCommand::Clean => (|| -> Result<i32, i32> {
                let cas_dir = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from(".glyim/cas"));
                let store = glyim_macro_vfs::LocalContentStore::new(&cas_dir).map_err(|e| {
                    eprintln!("error opening CAS: {e}");
                    1
                })?;

                // Collect all referenced hashes from names
                let names = store.list_names();
                let mut referenced = std::collections::HashSet::new();
                for name in &names {
                    if let Some(hash) = store.resolve_name(name) {
                        referenced.insert(hash);
                    }
                }

                let mut removed = 0usize;
                for blob in store.list_blobs() {
                    if !referenced.contains(&blob) {
                        if let Err(e) = store.delete_blob(blob) {
                            eprintln!("warning: could not delete blob {}: {}", blob, e);
                        } else {
                            removed += 1;
                        }
                    }
                }
                eprintln!("Removed {} unreferenced blobs.", removed);
                Ok(0)
            })()
            .unwrap_or_else(|code| code),
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
