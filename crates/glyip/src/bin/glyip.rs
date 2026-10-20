//! Binary entry point for the `glyip` build tool.

use clap::Parser;

/// Cargo-like build tool for the Glyim compiler.
#[derive(Parser, Debug)]
#[command(name = "glyip", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand, Debug)]
enum Commands {
    /// Create a new Glyim project.
    New {
        /// Project name.
        name: String,
        /// Create a library project.
        #[arg(long)]
        lib: bool,
        /// Language edition.
        #[arg(long, default_value = "2024")]
        edition: String,
    },
    /// Compile the project.
    Build {
        /// Build in release mode.
        #[arg(long)]
        release: bool,
        /// Target triple.
        #[arg(long)]
        target: Option<String>,
        /// Codegen backend: `llvm` (native executable, the default) or
        /// `bytecode` (glyim's own opcode format; not directly runnable).
        #[arg(long, default_value = "llvm")]
        backend: String,
        /// Optimisation level (0–3).
        #[arg(long, default_value_t = 0)]
        opt_level: u8,
        /// Link-time optimization: `off`, `thin`, or `fat` (Phase 10.2).
        #[arg(long, default_value = "off")]
        lto: String,
    },
    /// Run the project's tests.
    Test {
        /// Build in release mode.
        #[arg(long)]
        release: bool,
        /// Only run tests matching this substring.
        #[arg(long)]
        filter: Option<String>,
        /// Compile tests but don't run them.
        #[arg(long)]
        no_run: bool,
        /// Plan §23.1: compile each test file to a native executable and run it
        /// as a real subprocess instead of the in-process MIR interpreter.
        #[arg(long)]
        compiled: bool,
    },
    /// Build and execute the project's binary.
    Run {
        /// Build in release mode.
        #[arg(long)]
        release: bool,
        /// Arguments to pass to the binary.
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
        /// Target triple.
        #[arg(long)]
        target: Option<String>,
        /// Link-time optimization: `off`, `thin`, or `fat` (Phase 10.2).
        #[arg(long, default_value = "off")]
        lto: String,
    },
}

fn main() {
    // Initialise the tracing subscriber (minimal, to stderr).
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    let cli = Cli::parse();
    let project_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

    let result = match cli.command {
        Commands::New { name, lib, edition } => {
            let opts = glyip::config::NewOptions { lib, edition };
            glyip::cmd_new(&name, &opts, Some(&project_dir)).map(|_| ())
        }
        Commands::Build {
            release,
            target,
            backend,
            opt_level,
            lto,
        } => {
            let opts = glyip::config::BuildOptions {
                release,
                target,
                backend,
                opt_level,
                lto: parse_lto(&lto),
            };
            glyip::cmd_build(&project_dir, &opts).map(|_| ())
        }
        Commands::Test {
            release,
            filter,
            no_run,
            compiled,
        } => {
            let opts = glyip::config::TestOptions {
                release,
                filter,
                no_run,
                run_ignored: false,
                compiled,
            };
            glyip::cmd_test(&project_dir, &opts).map(|_| ())
        }
        Commands::Run {
            release,
            args,
            target,
            lto,
        } => {
            let opts = glyip::config::RunOptions {
                release,
                args,
                // A `run` needs something the OS can exec, which only the
                // LLVM backend produces.
                backend: "llvm".to_string(),
                target,
                lto: parse_lto(&lto),
            };
            // `glyip run` must exit with the *child's* status — it is a
            // transparent wrapper. Returning `Ok(())` here made `glyip run`
            // always exit 0, so a failing program looked like a passing
            // command.
            // `std::process::exit` has type `!`, so the `Ok` arm diverges and
            // the `match` yields the `Err` arm's type — no trailing expression
            // needed.
            match glyip::cmd_run(&project_dir, &opts) {
                Ok(run) => std::process::exit(run.exit_code),
                Err(e) => Err(e),
            }
        }
    };

    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

/// Translate the CLI `--lto` string (`off`/`thin`/`fat`) into an
/// `LtoKind`. Unknown values default to `None` (off) rather than failing the
/// whole build — the codegen side treats `None` as a no-op.
fn parse_lto(value: &str) -> Option<glyim_codegen_llvm::passes::LtoKind> {
    match value {
        "off" | "none" | "" => None,
        "thin" => Some(glyim_codegen_llvm::passes::LtoKind::Thin),
        "fat" => Some(glyim_codegen_llvm::passes::LtoKind::Fat),
        _ => None,
    }
}
