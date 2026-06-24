use clap::{Parser, Subcommand};
use pojoc_codegen::generate;
use pojoc_schema::ImportOrchestrator;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "pojoc", version, about = "Pojoc schema compiler")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Compile a schema and write output to disk
    Build {
        /// Entry-point .pojoc file
        input: PathBuf,

        /// Output directory for generated .rs files
        #[arg(short, long, default_value = "out")]
        out_dir: PathBuf,

        /// Print per-file diagnostics
        #[arg(short, long)]
        verbose: bool,
    },

    /// Check a schema without writing output
    Check {
        /// Entry-point .pojoc file
        input: PathBuf,

        /// Print per-file diagnostics
        #[arg(short, long)]
        verbose: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Build {
            input,
            out_dir,
            verbose,
        } => {
            build(input, out_dir, verbose);
        }
        Command::Check { input, verbose } => {
            check(input, verbose);
        }
    }
}

fn build(input: PathBuf, out_dir: PathBuf, _verbose: bool) {
    let file_target = out_dir.join(format!(
        "{}.rs",
        input.file_stem().unwrap().to_str().unwrap()
    ));

    if !file_target.exists() {
        std::fs::create_dir_all(&out_dir).unwrap();
    }

    let mut orchestrator = ImportOrchestrator::new();

    let schema = orchestrator.resolve_root(input.as_path()).unwrap();

    let code = generate(&schema);

    std::fs::write(file_target, code).unwrap();
}

fn check(input: PathBuf, _verbose: bool) {
    let mut orchestrator = ImportOrchestrator::new();

    orchestrator.resolve_root(input.as_path()).err();
}
