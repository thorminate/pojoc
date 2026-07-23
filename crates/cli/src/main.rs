use clap::{Parser, Subcommand};
use pojoc_build::codegen::generate;
use pojoc_build::schema::{AnalysisError, ImportOrchestrator};
use std::path::{Path, PathBuf};
use std::time::Instant;

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
        } => build(input, out_dir, verbose),
        Command::Check { input, verbose } => check(input, verbose),
    };
}

fn render_error(err: &AnalysisError, root: &Path) {
    eprint!("{}", err.render(root));
}

fn log(verbose: bool, msg: &str) {
    if verbose {
        println!("\x1b[2m  {msg}\x1b[0m");
    }
}

fn build(input: PathBuf, out_dir: PathBuf, verbose: bool) -> i32 {
    let mut orchestrator = ImportOrchestrator::new();

    let t = Instant::now();
    let schema = match orchestrator.resolve_root(input.as_path()) {
        Ok(s) => s,
        Err(e) => {
            render_error(&e, &input);
            return 1;
        }
    };
    log(
        verbose,
        &format!(
            "parsed & analyzed in {:.1}ms",
            t.elapsed().as_secs_f64() * 1000.0
        ),
    );

    let t = Instant::now();
    let code = generate(&schema);
    log(
        verbose,
        &format!("codegen in {:.1}ms", t.elapsed().as_secs_f64() * 1000.0),
    );

    if !out_dir.exists()
        && let Err(e) = std::fs::create_dir_all(&out_dir)
    {
        eprintln!(
            "error: could not create output dir `{}`: {e}",
            out_dir.display()
        );
        return 1;
    }

    let stem = input.file_stem().unwrap().to_string_lossy();
    let out_file = out_dir.join(format!("{stem}.rs"));

    let t = Instant::now();
    if let Err(e) = std::fs::write(&out_file, code) {
        eprintln!("error: could not write `{}`: {e}", out_file.display());
        return 1;
    }
    log(
        verbose,
        &format!(
            "wrote `{}` in {:.1}ms",
            out_file.display(),
            t.elapsed().as_secs_f64() * 1000.0
        ),
    );

    0
}

fn check(input: PathBuf, verbose: bool) -> i32 {
    let mut orchestrator = ImportOrchestrator::new();

    let t = Instant::now();
    match orchestrator.resolve_root(input.as_path()) {
        Ok(_) => {
            log(
                verbose,
                &format!(
                    "check passed in {:.1}ms",
                    t.elapsed().as_secs_f64() * 1000.0
                ),
            );
            0
        }
        Err(e) => {
            render_error(&e, &input);
            1
        }
    }
}
