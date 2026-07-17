use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{fs, io};

use crate::schema::ast::ImportDeclAst;
use crate::schema::error::AnalysisError;
use crate::schema::ir::analyzer::SchemaAnalyzer;
use crate::schema::ir::ir_types::ResolvedSchema;
use crate::schema::lexer::Lexer;
use crate::schema::parser::Parser;
use crate::schema::span::Span;
use crate::schema::{IndexableError, LoadError, SchemaAst};

pub struct ImportOrchestrator {
    cache: HashMap<PathBuf, Arc<ResolvedSchema>>,
    in_progress: Vec<PathBuf>,
}

impl ImportOrchestrator {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            in_progress: Vec::new(),
        }
    }

    #[allow(clippy::result_large_err)]
    pub fn resolve_root(&mut self, path: &Path) -> Result<Arc<ResolvedSchema>, AnalysisError> {
        let origin = path.to_path_buf();
        let canonical = self.canonicalize(path, &origin, Span::new(0, 0), 0)?;
        self.resolve_canonical(canonical, &origin, Span::new(0, 0), 0)
    }

    #[allow(clippy::result_large_err)]
    pub fn resolve_imports_for(
        &mut self,
        ast: &SchemaAst,
        own_path: &Path,
    ) -> Result<HashMap<String, Arc<ResolvedSchema>>, AnalysisError> {
        let canonical_self = fs::canonicalize(own_path).unwrap_or_else(|_| own_path.to_path_buf());
        self.in_progress.push(canonical_self.clone());

        let base_dir = own_path.parent().unwrap_or_else(|| Path::new("."));
        let mut imports = HashMap::new();

        for decl in &ast.imports {
            match self.resolve_import(own_path, base_dir, decl) {
                Ok(resolved) => {
                    imports.insert(decl.alias.clone(), resolved);
                }
                Err(e) => {
                    self.in_progress.retain(|p| p != &canonical_self);
                    return Err(e);
                }
            }
        }

        self.in_progress.retain(|p| p != &canonical_self);
        Ok(imports)
    }

    #[allow(clippy::result_large_err)]
    fn resolve_import(
        &mut self,
        importer: &Path,
        base_dir: &Path,
        decl: &ImportDeclAst,
    ) -> Result<Arc<ResolvedSchema>, AnalysisError> {
        let target = base_dir.join(&decl.path);
        let canonical = self.canonicalize(&target, importer, decl.span, decl.line)?;
        self.resolve_canonical(canonical, importer, decl.span, decl.line)
    }

    #[allow(clippy::result_large_err)]
    fn canonicalize(
        &self,
        path: &Path,
        importer: &Path,
        span: Span,
        line: u32,
    ) -> Result<PathBuf, AnalysisError> {
        fs::canonicalize(path).map_err(|_| AnalysisError::ImportNotFound {
            path: path.display().to_string(),
            origin: importer.to_path_buf(),
            span,
            line,
        })
    }

    #[allow(clippy::result_large_err)]
    fn resolve_canonical(
        &mut self,
        canonical: PathBuf,
        importer: &Path,
        span: Span,
        line: u32,
    ) -> Result<Arc<ResolvedSchema>, AnalysisError> {
        if let Some(cached) = self.cache.get(&canonical) {
            return Ok(cached.clone());
        }

        if let Some(idx) = self.in_progress.iter().position(|p| p == &canonical) {
            let chain = self.in_progress[idx..]
                .iter()
                .chain(std::iter::once(&canonical))
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(" → ");
            return Err(AnalysisError::CircularImport {
                chain,
                origin: importer.to_path_buf(),
                span,
                line,
            });
        }

        let path_str = canonical.display().to_string();
        let origin = importer.to_path_buf();
        let source = load_source(&canonical).map_err(|e| match e {
            LoadError::NotFound => AnalysisError::ImportNotFound {
                path: path_str.clone(),
                origin: origin.clone(),
                span,
                line,
            },
            LoadError::NotUtf8 { offset } => AnalysisError::ImportNotUtf8 {
                path: path_str.clone(),
                offset,
                origin: origin.clone(),
                span,
                line,
            },
            LoadError::Io { kind } => AnalysisError::ImportReadFailed {
                path: path_str.clone(),
                origin: origin.clone(),
                span,
                line,
                kind,
            },
        })?;

        self.in_progress.push(canonical.clone());
        let outcome = self.load_and_analyze(&canonical, &source);
        self.in_progress.pop();

        let resolved = outcome?;
        self.cache.insert(canonical, resolved.clone());
        Ok(resolved)
    }

    #[allow(clippy::result_large_err)]
    fn load_and_analyze(
        &mut self,
        canonical: &Path,
        source: &str,
    ) -> Result<Arc<ResolvedSchema>, AnalysisError> {
        let path_str = canonical.display().to_string();

        let tokens =
            Lexer::new(source)
                .tokenize()
                .map_err(|e| AnalysisError::ImportParseFailed {
                    path: path_str.clone(),
                    src: e.to_string(),
                    span: e.span(),
                    line: e.line(),
                })?;

        let ast =
            Parser::new(tokens)
                .parse_schema()
                .map_err(|e| AnalysisError::ImportParseFailed {
                    path: path_str.clone(),
                    src: e.to_string(),
                    span: e.span(),
                    line: e.line(),
                })?;

        let base_dir = canonical.parent().unwrap_or_else(|| Path::new("."));
        let mut imports = HashMap::new();
        for decl in &ast.imports {
            let resolved = self.resolve_import(canonical, base_dir, decl)?;
            imports.insert(decl.alias.clone(), resolved);
        }

        let mut analyzer = SchemaAnalyzer::new(&ast, imports);
        analyzer.run()?;
        Ok(Arc::new(analyzer.finish()?))
    }
}

impl Default for ImportOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

fn load_source(path: &Path) -> Result<String, LoadError> {
    let bytes = fs::read(path).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => LoadError::NotFound,
        kind => LoadError::Io { kind },
    })?;
    let mut s = String::from_utf8(bytes).map_err(|e| LoadError::NotUtf8 {
        offset: e.utf8_error().valid_up_to(),
    })?;
    if s.starts_with('\u{feff}') {
        s.drain(..3);
    }
    Ok(s)
}
