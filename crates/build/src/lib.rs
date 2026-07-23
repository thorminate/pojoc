//! Call pojoc from a `build.rs` without depending on separate schema/codegen crates.
//!
//! ```no_run
//! let out_dir = std::env::var("OUT_DIR").unwrap();
//! pojoc_build::compile_dir("schemas", &out_dir).unwrap_or_else(|e| panic!("\n{}", e.render()));
//! ```
//!
//! Writes one `<stem>.rs` per `.pojoc` file into `out_dir`, and emits the
//! `cargo:rerun-if-changed` directives so Cargo only re-runs the build script when a schema
//! actually changes. Include the generated file with
//! `include!(concat!(env!("OUT_DIR"), "/<stem>.rs"))`.

#[doc(hidden)]
pub mod codegen;
#[doc(hidden)]
pub mod core;
#[doc(hidden)]
pub mod schema;

use std::io;
use std::path::{Path, PathBuf};

use schema::ImportOrchestrator;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to compile schema {path}: {source}")]
    Analysis {
        path: PathBuf,
        source: Box<schema::AnalysisError>,
    },

    #[error("failed to read directory {path}: {source}")]
    ReadDir { path: PathBuf, source: io::Error },

    #[error("failed to write generated code to {path}: {source}")]
    Write { path: PathBuf, source: io::Error },

    #[error("schema path {0} has no file stem")]
    NoFileStem(PathBuf),
}

impl Error {
    /// Renders with source context where available: a `file:line:col` location
    /// and a caret at the offending span, matching `pojoc check`/`build`'s
    /// output. Other error kinds fall back to the plain message.
    pub fn render(&self) -> String {
        match self {
            Error::Analysis { path, source } => source.render(path),
            other => other.to_string(),
        }
    }
}

/// Compiles a single `.pojoc` file, writing `out_dir/<stem>.rs`. Returns the path written to.
pub fn compile(schema: impl AsRef<Path>, out_dir: impl AsRef<Path>) -> Result<PathBuf, Error> {
    let schema_path = schema.as_ref();
    println!("cargo:rerun-if-changed={}", schema_path.display());

    let mut orchestrator = ImportOrchestrator::new();
    let resolved = orchestrator
        .resolve_root(schema_path)
        .map_err(|source| Error::Analysis {
            path: schema_path.to_path_buf(),
            source: Box::new(source),
        })?;

    let code = codegen::generate(&resolved);

    let stem = schema_path
        .file_stem()
        .ok_or_else(|| Error::NoFileStem(schema_path.to_path_buf()))?;
    let out_path = out_dir.as_ref().join(stem).with_extension("rs");

    std::fs::write(&out_path, code).map_err(|source| Error::Write {
        path: out_path.clone(),
        source,
    })?;

    Ok(out_path)
}

/// Compiles every `.pojoc` file directly inside `schema_dir` (not recursive), writing
/// `out_dir/<stem>.rs` for each. Returns the generated paths in discovery (sorted) order.
pub fn compile_dir(
    schema_dir: impl AsRef<Path>,
    out_dir: impl AsRef<Path>,
) -> Result<Vec<PathBuf>, Error> {
    let schema_dir = schema_dir.as_ref();
    let out_dir = out_dir.as_ref();
    println!("cargo:rerun-if-changed={}", schema_dir.display());

    let mut entries: Vec<PathBuf> = std::fs::read_dir(schema_dir)
        .map_err(|source| Error::ReadDir {
            path: schema_dir.to_path_buf(),
            source,
        })?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "pojoc"))
        .collect();
    entries.sort();

    entries
        .into_iter()
        .map(|path| compile(path, out_dir))
        .collect()
}
