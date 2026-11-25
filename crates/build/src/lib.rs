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

pub use codegen::typescript::{
    GenerationOptions as TypeScriptOptions, RuntimeProfile as TypeScriptRuntime,
};

/// Overrides whether generated code emits `Serialize`/`Deserialize`, for
/// callers that decide this per invocation rather than via the `serde` cargo
/// feature. Returns the previous value.
pub use core::types::set_emit_serde;

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use schema::{ImportOrchestrator, ir_types::ResolvedSchema};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to compile schema {path}: {source}")]
    Analysis {
        path: PathBuf,
        source: Box<schema::AnalysisError>,
    },

    #[error("failed to read directory {path}: {source}")]
    ReadDir { path: PathBuf, source: io::Error },

    #[error("failed to generate TypeScript for schema {path}: {source}")]
    TypeScript {
        path: PathBuf,
        source: codegen::typescript::Error,
    },

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
    let resolved = resolve_for_build(schema_path)?;
    let out_path = out_dir
        .as_ref()
        .join(schema_stem(schema_path)?)
        .with_extension("rs");
    write_generated(&out_path, codegen::generate(&resolved))?;
    Ok(out_path)
}

/// Compiles a `.pojoc` schema to `out_dir/<stem>.ts` and returns the path.
/// The output directory is created as needed. Identical output is left alone
/// so running Cargo again does not trigger a frontend rebuild.
///
/// ```no_run
/// use pojoc_build::{TypeScriptOptions, compile_typescript};
///
/// compile_typescript(
///     "schemas/user.pojoc",
///     "web/generated",
///     TypeScriptOptions {
///         standalone: true,
///         ..TypeScriptOptions::default()
///     },
/// )?;
/// # Ok::<(), pojoc_build::Error>(())
/// ```
pub fn compile_typescript(
    schema: impl AsRef<Path>,
    out_dir: impl AsRef<Path>,
    options: TypeScriptOptions,
) -> Result<PathBuf, Error> {
    let schema_path = schema.as_ref();
    let out_dir = out_dir.as_ref();
    let resolved = resolve_for_build(schema_path)?;
    let mut file_name = schema_stem(schema_path)?.to_os_string();
    file_name.push(".ts");
    let out_path = out_dir.join(file_name);
    let code =
        codegen::typescript::generate_with_options(&resolved, options).map_err(|source| {
            Error::TypeScript {
                path: schema_path.to_path_buf(),
                source,
            }
        })?;
    if std::fs::read(&out_path).as_deref().ok() != Some(code.as_bytes()) {
        std::fs::create_dir_all(out_dir).map_err(|source| Error::Write {
            path: out_dir.to_path_buf(),
            source,
        })?;
        write_generated(&out_path, code)?;
    }
    Ok(out_path)
}

fn resolve_for_build(schema_path: &Path) -> Result<Arc<ResolvedSchema>, Error> {
    println!("cargo:rerun-if-changed={}", schema_path.display());

    let mut orchestrator = ImportOrchestrator::new();
    let resolved = orchestrator
        .resolve_root(schema_path)
        .map_err(|source| Error::Analysis {
            path: schema_path.to_path_buf(),
            source: Box::new(source),
        })?;

    let mut imports = orchestrator.source_paths().collect::<Vec<_>>();
    imports.sort_unstable();
    for path in imports {
        if Some(path) != resolved.source_path.as_deref() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }

    for (w, path) in resolved.warnings_recursive(schema_path) {
        println!("cargo::warning={}: {}", path.display(), w);
    }

    Ok(resolved)
}

fn schema_stem(schema_path: &Path) -> Result<&OsStr, Error> {
    schema_path
        .file_stem()
        .ok_or_else(|| Error::NoFileStem(schema_path.to_path_buf()))
}

fn write_generated(out_path: &Path, code: String) -> Result<(), Error> {
    std::fs::write(out_path, code).map_err(|source| Error::Write {
        path: out_path.to_path_buf(),
        source,
    })
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
