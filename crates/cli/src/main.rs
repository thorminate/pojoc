use clap::{Parser, Subcommand};
use pojoc_build::codegen::generate;
use pojoc_build::schema::{AnalysisError, ImportOrchestrator, IndexableError};
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

/// Converts an error to a path of a schema file, if it is an
/// import-related error, it returns the path of that schema file.
fn error_source_path<'a>(err: &'a AnalysisError, root: &'a Path) -> &'a Path {
    match err {
        AnalysisError::ImportParseFailed { path, .. } => Path::new(path.as_str()),
        AnalysisError::ImportNotFound { origin, .. }
        | AnalysisError::ImportNotUtf8 { origin, .. }
        | AnalysisError::ImportReadFailed { origin, .. }
        | AnalysisError::CircularImport { origin, .. } => origin.as_path(),
        _ => root,
    }
}

/// Emits a neatly formatted error to stdout.
fn render_error(err: &AnalysisError, root: &Path) {
    let source_path = error_source_path(err, root);
    let source = std::fs::read_to_string(source_path).unwrap_or_default();

    let line = err.line() as usize;
    let span = err.span();
    let message = err.to_string();

    // This is the path that will be displayed in the final output,
    // it is cleansed of unwanted symbols and stuff.
    let display_path = source_path
        .canonicalize()
        .unwrap_or_else(|_| source_path.to_path_buf());
    let display_path = display_path.display().to_string();
    let display_path = display_path.strip_prefix(r"\\?\").unwrap_or(&display_path);

    let lines: Vec<&str> = source.lines().collect();
    let line_idx = line.saturating_sub(1);
    let line_text = lines.get(line_idx).copied().unwrap_or("");

    // Gets a byte offset as to when the line starts.
    let line_start = line_start_offset(&source, line_idx);

    let col_start = span.start.saturating_sub(line_start);
    let col_end = span.end.saturating_sub(line_start);
    let caret_len = col_end.saturating_sub(col_start).max(1);

    let gutter = line.to_string();
    let pad = " ".repeat(gutter.len());

    // I don't feel like importing some ansi crate so raw ansi will do fine.
    eprintln!("\x1b[1;31merror\x1b[0m: {message}");
    eprintln!(" {pad}\x1b[34m-->\x1b[0m {display_path}:{line}:{col_start}");
    eprintln!(" {pad}\x1b[34m |\x1b[0m");
    eprintln!(" {gutter}\x1b[34m |\x1b[0m {line_text}");
    eprintln!(
        " {pad}\x1b[34m |\x1b[0m {}\x1b[1;31m{}\x1b[0m",
        " ".repeat(col_start),
        "^".repeat(caret_len),
    );
}

/// Gets the byte offset of a string for the start of a certain line.
fn line_start_offset(source: &str, line_idx: usize) -> usize {
    if line_idx == 0 {
        return 0;
    }
    let mut seen = 0;
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            seen += 1;
            if seen == line_idx {
                return i + 1;
            }
        }
    }
    source.len()
}

/// Simply emits a log to stdout with a dark gray ansi
/// color encoding and 2 whitespaces of filler.
fn log(verbose: bool, msg: &str) {
    if verbose {
        println!("\x1b[2m  {msg}\x1b[0m");
    }
}

/// Builds a schema file into Rust code with conditionally verbose
/// time diagnostics. Also returning an error code if something went wrong.
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

/// Runs the analysis and resolution steps with no codegen and output.
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
