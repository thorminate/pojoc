use crate::schema::lexer::Token;
use crate::schema::span::Span;
use std::io;
use std::path::PathBuf;
use thiserror::Error;

pub trait IndexableError {
    fn span(&self) -> Span;
    fn line(&self) -> u32;
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("unexpected token `{got}`, expected {expected}")]
    UnexpectedToken {
        got: Token,
        expected: String,
        span: Span,
        line: u32,
    },
    #[error("unexpected EOF")]
    UnexpectedEof,
    #[error("{message}")]
    InvalidSyntax {
        message: String,
        span: Span,
        line: u32,
    },
}

impl IndexableError for ParseError {
    fn span(&self) -> Span {
        match self {
            ParseError::UnexpectedToken { span, .. } => *span,
            ParseError::UnexpectedEof => Span::new(0, 0),
            ParseError::InvalidSyntax { span, .. } => *span,
        }
    }

    fn line(&self) -> u32 {
        match self {
            ParseError::UnexpectedToken { line, .. } => *line,
            ParseError::UnexpectedEof => 0,
            ParseError::InvalidSyntax { line, .. } => *line,
        }
    }
}

#[derive(Debug, Error)]
pub enum LexError {
    #[error("unexpected character `{ch}`")]
    UnexpectedChar { ch: char, span: Span, line: u32 },
}

impl IndexableError for LexError {
    fn span(&self) -> Span {
        match self {
            LexError::UnexpectedChar { span, .. } => *span,
        }
    }

    fn line(&self) -> u32 {
        match self {
            LexError::UnexpectedChar { line, .. } => *line,
        }
    }
}

#[derive(Debug, Error)]
pub enum AnalysisError {
    #[error("unknown type '{name}' referenced in version {version}")]
    UnknownType {
        name: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("type '{child}' in version {version} extends unknown type '{parent}'")]
    UnknownParentType {
        child: String,
        parent: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "generic type '{name}' in version {version} expects {expected} type argument(s), got {found}"
    )]
    GenericArityMismatch {
        name: String,
        expected: usize,
        found: usize,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "generic instantiation '{name}' in version {version} collides with an existing type of the same name"
    )]
    GenericNameCollision {
        name: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "type '{type_name}' in version {version} drops a type parameter via `_` but field '{field}' still references it — remove or retype it in this diff"
    )]
    UnresolvedGenericWildcard {
        field: String,
        type_name: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("version {version}: enum '{type_name}' has no variant '{variant}'")]
    UnknownEnumVariant {
        type_name: String,
        variant: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("type '{name}' in version {version} uses 'extends' but body must be a diff")]
    ExtendsWithFullDefinition {
        name: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "diff op '{op}' references unknown field '{field}' in type '{type_name}' version {version}"
    )]
    FieldNotFound {
        op: &'static str,
        field: String,
        type_name: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("field '{field}' in version {version} doesn't have a default value")]
    MissingDefault {
        field: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "field '{field}' in version {version} has a fixed {kind} of {expected} entries but default value has {got} entries"
    )]
    FixedSizeDefaultLengthMismatch {
        field: String,
        kind: &'static str,
        expected: usize,
        got: usize,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("version {version}: diff adds field `{field}` but it already exists")]
    FieldAlreadyExists {
        version: i128,
        field: String,
        span: Span,
        line: u32,
    },

    #[error(
        "field '{field}' in version {version} has a fixed string of {expected} bytes but default value is {got} bytes"
    )]
    FixedStringDefaultLengthMismatch {
        field: String,
        expected: usize,
        got: usize,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "version {version}: {kind} fixed size {n} is too large, consider not using a fixed size"
    )]
    FixedSizeTooLarge {
        kind: &'static str,
        n: usize,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("version {version}: type mismatch: expected {expected}, got {got}")]
    TypeMismatch {
        expected: String,
        got: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("version {version}: varints cannot be used in const fields")]
    VarintsCannotBeConst {
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("version {version}: invalid vfloat: {reason}")]
    InvalidVFloat {
        reason: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("version {version}: vfloat range is too large: {range}")]
    VFloatRangeTooLarge {
        range: f64,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "field '{field}' in version {version}'s default value {value} is out of range (min: {min}, max: {max})"
    )]
    VFloatDefaultOutOfRange {
        field: String,
        value: f64,
        min: f64,
        max: f64,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "field '{field}' in version {version}'s default value {value} is out of range (min: {min}, max: {max})"
    )]
    IntDefaultOutOfRange {
        field: String,
        value: i128,
        min: i128,
        max: i128,
        type_name: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "version {version}: `(delta)` can only be applied to integer array elements (u8/u16/u32/u64/i8/i16/i32/i64), not `{type_desc}`"
    )]
    InvalidDeltaElementType {
        type_desc: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "version {version}: reserved variant name `{name}` cannot be used in type `{type_name}`"
    )]
    ReservedVariantName {
        name: String,
        type_name: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "version {version}: lazy field '{field}' added via diff must be optional (`lazy T?`) so older messages can default to None"
    )]
    LazyDiffFieldMustBeOptional {
        field: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "version {version}: field '{field}' in type '{type_name}': `seekable` is only allowed on root schema fields, since accessors are generated against the schema's own per-version decoders"
    )]
    SeekableOnlyOnRootFields {
        field: String,
        type_name: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("unknown import alias '{alias}'")]
    UnknownImportAlias {
        alias: String,
        span: Span,
        line: u32,
    },

    #[error(
        "import '{alias}' references version {version} but schema only has versions up to {max}"
    )]
    ImportVersionOutOfRange {
        alias: String,
        version: i128,
        max: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "{}",
        if *is_root {
            format!("could not find file '{path}'")
        } else {
            format!("import path '{path}' could not be found")
        }
    )]
    ImportNotFound {
        path: String,
        is_root: bool,
        origin: PathBuf,
        span: Span,
        line: u32,
    },

    #[error(
        "{}",
        if *is_root {
            format!("file '{path}' is not valid UTF-8 (invalid byte at offset {offset})")
        } else {
            format!("import is not valid UTF-8: {path} (invalid byte at offset {offset})")
        }
    )]
    ImportNotUtf8 {
        path: String,
        offset: usize,
        is_root: bool,
        origin: PathBuf,
        span: Span,
        line: u32,
    },

    #[error(
        "{}",
        if *is_root {
            format!("failed to read file '{path}': {kind}")
        } else {
            format!("failed to read import '{path}': {kind}")
        }
    )]
    ImportReadFailed {
        path: String,
        is_root: bool,
        origin: PathBuf,
        span: Span,
        line: u32,
        kind: io::ErrorKind,
    },

    #[error("circular import detected: {chain}")]
    CircularImport {
        chain: String,
        origin: PathBuf,
        span: Span,
        line: u32,
    },

    #[error("{src}")]
    ImportParseFailed {
        path: String,
        src: String,
        span: Span,
        line: u32,
    },

    #[error("schema must have at least one version")]
    NoVersions { span: Span, line: u32 },

    #[error(
        "type '{name}' in version {version} has the same name as its enclosing schema '{name}' — rename one of them"
    )]
    TypeNameShadowsSchema {
        name: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("'box' is a reserved builtin type and cannot be aliased with `as`, or redeclared")]
    InvalidBoxUsage { span: Span, line: u32 },

    #[error("version {version}: invalid constraint: {reason}")]
    InvalidConstraint {
        reason: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("version {version}: invalid `intern`: {reason}")]
    InvalidIntern {
        reason: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "type '{type_name}' in version {version} is recursive without `box<>`: {cycle} forms a cycle — wrap at least one field in `box<T>` to break it"
    )]
    UnboxedRecursiveType {
        type_name: String,
        cycle: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "version {version}: field '{field}' can't be retyped from '{old_type}' to unrelated type '{new_type}' — add a new field instead"
    )]
    IncompatibleStructRetype {
        field: String,
        old_type: String,
        new_type: String,
        version: i128,
        span: Span,
        line: u32,
    },
}

impl IndexableError for AnalysisError {
    fn span(&self) -> Span {
        match self {
            AnalysisError::UnknownType { span, .. } => *span,
            AnalysisError::GenericArityMismatch { span, .. } => *span,
            AnalysisError::GenericNameCollision { span, .. } => *span,
            AnalysisError::UnresolvedGenericWildcard { span, .. } => *span,
            AnalysisError::UnknownParentType { span, .. } => *span,
            AnalysisError::UnknownEnumVariant { span, .. } => *span,
            AnalysisError::ExtendsWithFullDefinition { span, .. } => *span,
            AnalysisError::FieldNotFound { span, .. } => *span,
            AnalysisError::MissingDefault { span, .. } => *span,
            AnalysisError::FixedSizeDefaultLengthMismatch { span, .. } => *span,
            AnalysisError::FieldAlreadyExists { span, .. } => *span,
            AnalysisError::FixedStringDefaultLengthMismatch { span, .. } => *span,
            AnalysisError::FixedSizeTooLarge { span, .. } => *span,
            AnalysisError::TypeMismatch { span, .. } => *span,
            AnalysisError::VarintsCannotBeConst { span, .. } => *span,
            AnalysisError::InvalidVFloat { span, .. } => *span,
            AnalysisError::VFloatRangeTooLarge { span, .. } => *span,
            AnalysisError::VFloatDefaultOutOfRange { span, .. } => *span,
            AnalysisError::IntDefaultOutOfRange { span, .. } => *span,
            AnalysisError::InvalidDeltaElementType { span, .. } => *span,
            AnalysisError::ReservedVariantName { span, .. } => *span,
            AnalysisError::LazyDiffFieldMustBeOptional { span, .. } => *span,
            AnalysisError::SeekableOnlyOnRootFields { span, .. } => *span,
            AnalysisError::UnknownImportAlias { span, .. } => *span,
            AnalysisError::ImportVersionOutOfRange { span, .. } => *span,
            AnalysisError::ImportNotFound { span, .. } => *span,
            AnalysisError::ImportNotUtf8 { span, .. } => *span,
            AnalysisError::ImportReadFailed { span, .. } => *span,
            AnalysisError::CircularImport { span, .. } => *span,
            AnalysisError::ImportParseFailed { span, .. } => *span,
            AnalysisError::NoVersions { span, .. } => *span,
            AnalysisError::TypeNameShadowsSchema { span, .. } => *span,
            AnalysisError::InvalidBoxUsage { span, .. } => *span,
            AnalysisError::UnboxedRecursiveType { span, .. } => *span,
            AnalysisError::InvalidConstraint { span, .. } => *span,
            AnalysisError::InvalidIntern { span, .. } => *span,
            AnalysisError::IncompatibleStructRetype { span, .. } => *span,
        }
    }

    fn line(&self) -> u32 {
        match self {
            AnalysisError::UnknownType { line, .. } => *line,
            AnalysisError::GenericArityMismatch { line, .. } => *line,
            AnalysisError::GenericNameCollision { line, .. } => *line,
            AnalysisError::UnresolvedGenericWildcard { line, .. } => *line,
            AnalysisError::UnknownParentType { line, .. } => *line,
            AnalysisError::UnknownEnumVariant { line, .. } => *line,
            AnalysisError::ExtendsWithFullDefinition { line, .. } => *line,
            AnalysisError::FieldNotFound { line, .. } => *line,
            AnalysisError::MissingDefault { line, .. } => *line,
            AnalysisError::FixedSizeDefaultLengthMismatch { line, .. } => *line,
            AnalysisError::FieldAlreadyExists { line, .. } => *line,
            AnalysisError::FixedStringDefaultLengthMismatch { line, .. } => *line,
            AnalysisError::FixedSizeTooLarge { line, .. } => *line,
            AnalysisError::TypeMismatch { line, .. } => *line,
            AnalysisError::VarintsCannotBeConst { line, .. } => *line,
            AnalysisError::InvalidVFloat { line, .. } => *line,
            AnalysisError::VFloatRangeTooLarge { line, .. } => *line,
            AnalysisError::VFloatDefaultOutOfRange { line, .. } => *line,
            AnalysisError::IntDefaultOutOfRange { line, .. } => *line,
            AnalysisError::InvalidDeltaElementType { line, .. } => *line,
            AnalysisError::ReservedVariantName { line, .. } => *line,
            AnalysisError::LazyDiffFieldMustBeOptional { line, .. } => *line,
            AnalysisError::SeekableOnlyOnRootFields { line, .. } => *line,
            AnalysisError::UnknownImportAlias { line, .. } => *line,
            AnalysisError::ImportVersionOutOfRange { line, .. } => *line,
            AnalysisError::ImportNotFound { line, .. } => *line,
            AnalysisError::ImportNotUtf8 { line, .. } => *line,
            AnalysisError::ImportReadFailed { line, .. } => *line,
            AnalysisError::CircularImport { line, .. } => *line,
            AnalysisError::ImportParseFailed { line, .. } => *line,
            AnalysisError::NoVersions { line, .. } => *line,
            AnalysisError::TypeNameShadowsSchema { line, .. } => *line,
            AnalysisError::InvalidBoxUsage { line, .. } => *line,
            AnalysisError::UnboxedRecursiveType { line, .. } => *line,
            AnalysisError::InvalidConstraint { line, .. } => *line,
            AnalysisError::InvalidIntern { line, .. } => *line,
            AnalysisError::IncompatibleStructRetype { line, .. } => *line,
        }
    }
}

impl AnalysisError {
    /// the file this error is reported against, usually root but import errors
    /// point at the importing/imported file instead
    fn source_path<'a>(&'a self, root: &'a std::path::Path) -> &'a std::path::Path {
        match self {
            AnalysisError::ImportParseFailed { path, .. } => std::path::Path::new(path.as_str()),
            AnalysisError::ImportNotFound { origin, .. }
            | AnalysisError::ImportNotUtf8 { origin, .. }
            | AnalysisError::ImportReadFailed { origin, .. }
            | AnalysisError::CircularImport { origin, .. } => origin.as_path(),
            _ => root,
        }
    }

    /// renders with a file:line:col location and caret, matching pojoc check/build.
    /// reads the source file from disk, so it may diverge if the file changed since
    pub fn render(&self, root: &std::path::Path) -> String {
        render_snippet(
            "error",
            &self.to_string(),
            self.line() as usize,
            self.span(),
            self.source_path(root),
        )
    }
}

/// shared by errors and warnings: a `severity: message` line, then the source
/// line with a caret under the span
pub(crate) fn render_snippet(
    severity: &str,
    message: &str,
    line: usize,
    span: Span,
    source_path: &std::path::Path,
) -> String {
    {
        use std::fmt::Write;

        // line 0 means there's no real source location (file couldn't be
        // loaded at all — not found, unreadable, not UTF-8) — a snippet/caret
        // pointing at nothing is worse than no snippet at all.
        if line == 0 {
            return format!("{severity}: {message}\n");
        }

        let source = std::fs::read_to_string(source_path).unwrap_or_default();

        let display_path = source_path
            .canonicalize()
            .unwrap_or_else(|_| source_path.to_path_buf());
        let display_path = display_path.display().to_string();
        let display_path = display_path.strip_prefix(r"\\?\").unwrap_or(&display_path);

        let lines: Vec<&str> = source.lines().collect();
        let line_idx = line.saturating_sub(1);
        let line_text = lines.get(line_idx).copied().unwrap_or("");

        let mut line_start = 0;
        if line_idx > 0 {
            let mut seen = 0;
            for (i, b) in source.bytes().enumerate() {
                if b == b'\n' {
                    seen += 1;
                    if seen == line_idx {
                        line_start = i + 1;
                        break;
                    }
                }
            }
        }

        let col_start = span.start.saturating_sub(line_start);
        let col_end = span.end.saturating_sub(line_start);
        let caret_len = col_end.saturating_sub(col_start).max(1);

        let gutter = line.to_string();
        let pad = " ".repeat(gutter.len());

        let mut out = String::new();
        let _ = writeln!(out, "{severity}: {message}");
        let _ = writeln!(out, " {pad} --> {display_path}:{line}:{col_start}");
        let _ = writeln!(out, " {pad} |");
        let _ = writeln!(out, " {gutter} | {line_text}");
        let _ = writeln!(
            out,
            " {pad} | {}{}",
            " ".repeat(col_start),
            "^".repeat(caret_len)
        );
        out
    }
}

/// non-fatal schema diagnostics. unlike AnalysisError these never fail a build —
/// they flag a schema-level choice whose cost isn't visible from the source.
#[derive(Debug, Clone, Error)]
pub enum SchemaWarning {
    #[error(
        "version {version}: field '{field}' is `seekable` and reaches the intern table through a container or named type, so seeking it materializes the whole table. a bare `seekable intern string` resolves a single index instead"
    )]
    SeekMaterializesInternTable {
        field: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "version {version}: field '{field}' was `seekable` in version {was} but this `~` does not restate it, so its accessors will no longer be generated. write `~ {field}: seekable ...` to keep them"
    )]
    SeekableDroppedByDiff {
        field: String,
        was: i128,
        version: i128,
        span: Span,
        line: u32,
    },
}

impl SchemaWarning {
    pub fn span(&self) -> Span {
        match self {
            SchemaWarning::SeekMaterializesInternTable { span, .. } => *span,
            SchemaWarning::SeekableDroppedByDiff { span, .. } => *span,
        }
    }

    pub fn line(&self) -> u32 {
        match self {
            SchemaWarning::SeekMaterializesInternTable { line, .. } => *line,
            SchemaWarning::SeekableDroppedByDiff { line, .. } => *line,
        }
    }

    /// same layout as AnalysisError::render, with a `warning:` severity
    pub fn render(&self, root: &std::path::Path) -> String {
        render_snippet(
            "warning",
            &self.to_string(),
            self.line() as usize,
            self.span(),
            root,
        )
    }
}

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("file not found")]
    NotFound,
    #[error("not valid UTF-8 (invalid byte at offset {offset})")]
    NotUtf8 { offset: usize },
    #[error("read failed: {kind}")]
    Io { kind: io::ErrorKind },
}

impl IndexableError for LoadError {
    fn line(&self) -> u32 {
        match self {
            LoadError::NotFound => 0,
            LoadError::NotUtf8 { .. } => 0,
            LoadError::Io { .. } => 0,
        }
    }

    fn span(&self) -> Span {
        match self {
            LoadError::NotFound => Span::new(0, 0),
            LoadError::NotUtf8 { .. } => Span::new(0, 0),
            LoadError::Io { .. } => Span::new(0, 0),
        }
    }
}

#[derive(Debug, Error)]
pub enum SchemaError {
    #[error(transparent)]
    Lex(#[from] LexError),
    #[error(transparent)]
    Parse(#[from] ParseError),
    #[error(transparent)]
    Analysis(#[from] AnalysisError),
    #[error(transparent)]
    Load(#[from] LoadError),
}
