use clap::{Parser, Subcommand};
use pojoc_codegen::generate;
use pojoc_schema::{AnalysisError, ImportOrchestrator, IndexableError};
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
        } => {
            build(input, out_dir, verbose);
        }
        Command::Check { input, verbose } => {
            check(input, verbose);
        }
    }
}

fn error_source_path<'a>(err: &'a AnalysisError, root: &'a Path) -> &'a Path {
    match err {
        AnalysisError::ImportParseFailed { path, .. }
        | AnalysisError::ImportNotFound { path, .. }
        | AnalysisError::CircularImport { path, .. } => Path::new(path.as_str()),
        _ => root,
    }
}

fn render_error(err: &AnalysisError, root: &Path) {
    let source_path = error_source_path(err, root);
    let source = std::fs::read_to_string(source_path).unwrap_or_default();

    let line = err.line() as usize;
    let span = err.span();
    let message = err.to_string();

    let display_path = source_path
        .canonicalize()
        .unwrap_or_else(|_| source_path.to_path_buf());
    let display_path = display_path.display().to_string();
    let display_path = display_path.strip_prefix(r"\\?\").unwrap_or(&display_path);

    let line_idx = line.saturating_sub(1);
    let lines: Vec<&str> = source.lines().collect();
    let line_text = lines.get(line_idx).copied().unwrap_or("");

    let line_start = line_start_offset(&source, line_idx);

    let col_start = span.start.saturating_sub(line_start);
    let col_end = span.end.saturating_sub(line_start);
    let caret_len = col_end.saturating_sub(col_start).max(1);

    let gutter = line.to_string();
    let pad = " ".repeat(gutter.len());

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

fn line_start_offset(source: &str, line_idx: usize) -> usize {
    let mut current_line = 0;
    let mut offset = 0;
    for b in source.bytes() {
        if current_line == line_idx {
            break;
        }
        offset += 1;
        if b == b'\n' {
            current_line += 1;
        }
    }
    offset
}

fn log(verbose: bool, msg: &str) {
    if verbose {
        eprintln!("\x1b[2m  {msg}\x1b[0m");
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

    if !out_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&out_dir) {
            eprintln!(
                "error: could not create output dir `{}`: {e}",
                out_dir.display()
            );
            return 1;
        }
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
