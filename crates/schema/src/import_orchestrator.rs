use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::ast::ImportDeclAst;
use crate::error::AnalysisError;
use crate::ir::analyzer::SchemaAnalyzer;
use crate::ir::ir_types::ResolvedSchema;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::span::Span;
use crate::{SchemaAst, SchemaError};
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
    pub fn resolve_root(&mut self, path: &Path) -> Result<Arc<ResolvedSchema>, SchemaError> {
        let canonical = self.canonicalize(path, Span::new(0, 0), 0)?;
        self.resolve_canonical(canonical, Span::new(0, 0), 0)
    }

    #[allow(clippy::result_large_err)]
    pub fn resolve_imports_for(
        &mut self,
        ast: &SchemaAst,
        own_path: &Path,
    ) -> Result<HashMap<String, Arc<ResolvedSchema>>, SchemaError> {
        let canonical_self = fs::canonicalize(own_path).unwrap_or_else(|_| own_path.to_path_buf());
        self.in_progress.push(canonical_self.clone());

        let base_dir = own_path.parent().unwrap_or_else(|| Path::new("."));
        let mut imports = HashMap::new();

        for decl in &ast.imports {
            match self.resolve_import(base_dir, decl) {
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
        base_dir: &Path,
        decl: &ImportDeclAst,
    ) -> Result<Arc<ResolvedSchema>, SchemaError> {
        let target = base_dir.join(&decl.path);
        let canonical = self.canonicalize(&target, decl.span, decl.line)?;
        self.resolve_canonical(canonical, decl.span, decl.line)
    }

    #[allow(clippy::result_large_err)]
    fn canonicalize(&self, path: &Path, span: Span, line: u32) -> Result<PathBuf, AnalysisError> {
        fs::canonicalize(path).map_err(|_| AnalysisError::ImportNotFound {
            path: path.display().to_string(),
            span,
            line,
        })
    }

    #[allow(clippy::result_large_err)]
    fn resolve_canonical(
        &mut self,
        canonical: PathBuf,
        span: Span,
        line: u32,
    ) -> Result<Arc<ResolvedSchema>, SchemaError> {
        if let Some(cached) = self.cache.get(&canonical) {
            return Ok(cached.clone());
        }

        if self.in_progress.contains(&canonical) {
            return Err(SchemaError::Analysis(AnalysisError::CircularImport {
                path: canonical.display().to_string(),
                span,
                line,
            }));
        }

        self.in_progress.push(canonical.clone());
        let outcome = self.load_and_analyze(&canonical);
        self.in_progress.pop();

        let resolved = outcome?;
        self.cache.insert(canonical, resolved.clone());
        Ok(resolved)
    }

    #[allow(clippy::result_large_err)]
    fn load_and_analyze(&mut self, canonical: &Path) -> Result<Arc<ResolvedSchema>, SchemaError> {
        let path_str = canonical.display().to_string();

        let raw = fs::read_to_string(canonical).map_err(|_| {
            SchemaError::Analysis(AnalysisError::ImportNotFound {
                path: path_str.clone(),
                span: Span::new(0, 0),
                line: 0,
            })
        })?;
        let source = raw.strip_prefix('\u{feff}').unwrap_or(&raw);

        let tokens = Lexer::new(source).tokenize().map_err(SchemaError::Lex)?;

        let ast = Parser::new(tokens)
            .parse_schema()
            .map_err(SchemaError::Parse)?;

        let base_dir = canonical.parent().unwrap_or_else(|| Path::new("."));
        let mut imports = HashMap::new();
        for decl in &ast.imports {
            let resolved = self.resolve_import(base_dir, decl)?;
            imports.insert(decl.alias.clone(), resolved);
        }

        let mut analyzer = SchemaAnalyzer::new(&ast, imports);
        analyzer.run()?;
        let resolved = analyzer.finish()?;
        Ok(Arc::new(resolved))
    }
}

impl Default for ImportOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}
