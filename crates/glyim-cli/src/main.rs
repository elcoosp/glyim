use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};
use glyim_cli::pipeline;

#[derive(Parser)]
#[command(name = "glyim", version, about = "The Glyim compiler")]
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
    },
    Run {
        input: PathBuf,
    },
    Ir {
        input: PathBuf,
    },
    Check {
        input: PathBuf,
    },
    Export {
        name: String,
        dest: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();
    let exit_code = match cli.command {
        Command::Build { input, output } => match pipeline::build(&input, output.as_deref()) {
            Ok(path) => { eprintln!("Built: {}", path.display()); 0 }
            Err(e) => { eprintln!("error: {e}"); 1 }
        }
        Command::Run { input } => match pipeline::run(&input) {
            Ok(code) => code,
            Err(e) => { eprintln!("error: {e}"); 1 }
        }
        Command::Ir { input } => match pipeline::print_ir(&input) {
            Ok(()) => 0,
            Err(e) => { eprintln!("error: {e}"); 1 }
        }
        Command::Check { input } => match pipeline::check(&input) {
            Ok(()) => 0,
            Err(e) => { eprintln!("error: {e}"); 1 }
        }
        Command::Export { name, dest } => {
            eprintln!("error: 'export' not implemented in v0.1.0 (artifact: {name}, dest: {})", dest.display());
            1
        }
    };
    process::exit(exit_code);
}
