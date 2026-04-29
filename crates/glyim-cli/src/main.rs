use clap::{Parser, Subcommand};
use glyim_cli::pipeline::{self, BuildMode};
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
}

fn main() {
    let cli = Cli::parse();
    let exit_code = match cli.command {
        Command::Build { input, output, debug, release } => {
            let mode = if release { BuildMode::Release } else { BuildMode::Debug };
            match pipeline::build_with_mode(&input, output.as_deref(), mode) {
                Ok(path) => {
                    eprintln!("Built: {}", path.display());
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        },
        Command::Run { input, debug, release } => {
            let mode = if release { BuildMode::Release } else { BuildMode::Debug };
            match pipeline::run_with_mode(&input, mode) {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        },
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
        Command::Export { name, dest } => {
            eprintln!(
                "error: 'export' not implemented (artifact: {name}, dest: {})",
                dest.display()
            );
            1
        }
    };
    process::exit(exit_code);
}
