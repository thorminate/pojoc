mod completion;

use crate::completion::{SchemaIndex, completions_for_position};
use lsp_server::*;
use lsp_types::*;
use pojoc_schema::analyzer::SchemaAnalyzer;
use pojoc_schema::{
    AnalysisError, ImportOrchestrator, IndexableError, Lexer, LineIndex, ParseError, Parser,
    Position as SchemaPosition, SchemaAst, SchemaError, Span,
};
use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};

struct DocStore {
    docs: HashMap<Uri, String>,
    last_good_ast: HashMap<Uri, SchemaAst>,
    import_versions: HashMap<Uri, HashMap<String, Vec<i128>>>,
}

impl DocStore {
    fn new() -> Self {
        Self {
            docs: HashMap::new(),
            last_good_ast: HashMap::new(),
            import_versions: HashMap::new(),
        }
    }
    fn set(&mut self, uri: Uri, text: String) {
        self.docs.insert(uri, text);
    }
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let (connection, io_threads) = Connection::stdio();

    let server_capabilities = serde_json::to_value(ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![
                ":".into(),
                "@".into(),
                "<".into(),
                "[".into(),
                "(".into(),
                "-".into(),
                "~".into(),
                ",".into(),
                "=".into(),
                " ".into(),
            ]),
            ..Default::default()
        }),
        ..Default::default()
    })?;

    let _initialization_params = connection.initialize(server_capabilities)?;
    let mut store = DocStore::new();

    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    break;
                }
                if req.method == "textDocument/completion" {
                    let items = (|| -> Option<Vec<CompletionItem>> {
                        let params: CompletionParams =
                            serde_json::from_value(req.params.clone()).ok()?;
                        let uri = &params.text_document_position.text_document.uri;
                        let pos = params.text_document_position.position;
                        let text = store.docs.get(uri)?;
                        let offset = position_to_offset(text, pos);

                        let mut idx = store
                            .last_good_ast
                            .get(uri)
                            .map(SchemaIndex::build)
                            .unwrap_or_default();
                        if let Some(versions) = store.import_versions.get(uri) {
                            idx.import_versions = versions.clone();
                        }

                        let schema_path = uri_to_path(uri);
                        Some(completions_for_position(
                            text,
                            offset,
                            &idx,
                            schema_path.as_deref(),
                        ))
                    })()
                    .unwrap_or_default();

                    let result = serde_json::to_value(CompletionResponse::Array(items))?;
                    connection.sender.send(Message::Response(Response {
                        id: req.id,
                        result: Some(result),
                        error: None,
                    }))?;
                }
            }
            Message::Notification(notif) => match notif.method.as_str() {
                "textDocument/didOpen" => {
                    match serde_json::from_value::<DidOpenTextDocumentParams>(notif.params) {
                        Ok(params) => {
                            let uri = params.text_document.uri;
                            let text = params.text_document.text;
                            if let Err(e) = handle_text_update(&mut store, uri, text, &connection) {
                                eprintln!("failed to handle didOpen: {e}");
                            }
                        }
                        Err(e) => eprintln!("malformed didOpen params: {e}"),
                    }
                }
                "textDocument/didChange" => {
                    match serde_json::from_value::<DidChangeTextDocumentParams>(notif.params) {
                        Ok(params) => {
                            let uri = params.text_document.uri;
                            if let Some(change) = params.content_changes.into_iter().last() {
                                if let Err(e) =
                                    handle_text_update(&mut store, uri, change.text, &connection)
                                {
                                    eprintln!("failed to handle didChange: {e}");
                                }
                            }
                        }
                        Err(e) => eprintln!("malformed didChange params: {e}"),
                    }
                }
                "textDocument/didClose" => {
                    if let Ok(params) =
                        serde_json::from_value::<DidCloseTextDocumentParams>(notif.params)
                    {
                        store.docs.remove(&params.text_document.uri);
                    }
                }
                _ => {}
            },
            Message::Response(_) => {}
        }
    }

    io_threads.join()?;
    Ok(())
}

fn publish_diagnostics(
    connection: &Connection,
    uri: Uri,
    diagnostics: Vec<Diagnostic>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let params = PublishDiagnosticsParams {
        uri,
        diagnostics,
        version: None,
    };
    connection
        .sender
        .send(Message::Notification(Notification::new(
            "textDocument/publishDiagnostics".to_string(),
            params,
        )))?;
    Ok(())
}

fn error_to_diagnostic(err: &SchemaError, text: &str, line_index: &LineIndex) -> Diagnostic {
    let span = extract_span(err, text);
    Diagnostic {
        range: span_to_range(text, span, line_index),
        severity: Some(DiagnosticSeverity::ERROR),
        message: err.to_string(),
        ..Default::default()
    }
}

fn extract_span(err: &SchemaError, text: &str) -> Span {
    match err {
        SchemaError::Lex(e) => e.span(),
        SchemaError::Parse(ParseError::UnexpectedEof) => {
            let end = text.len();
            Span::new(end, end)
        }
        SchemaError::Parse(e) => e.span(),
        SchemaError::Analysis(e) => e.span(),
    }
}

fn span_to_range(text: &str, span: Span, line_index: &LineIndex) -> Range {
    Range {
        start: to_lsp_position(line_index.position(text, span.start)),
        end: to_lsp_position(line_index.position(text, span.end)),
    }
}

fn to_lsp_position(pos: SchemaPosition) -> Position {
    Position {
        line: pos.line,
        character: pos.character,
    }
}

fn parse_ast(source: &str) -> Result<SchemaAst, SchemaError> {
    let tokens = Lexer::new(source).tokenize()?;
    Parser::new(tokens)
        .parse_schema()
        .map_err(SchemaError::from)
}

fn analyze(
    ast: &SchemaAst,
    own_path: &Path,
    store: &mut DocStore,
    uri: &Uri,
) -> Result<(), SchemaError> {
    let mut orchestrator = ImportOrchestrator::new();
    let imports = orchestrator.resolve_imports_for(ast, own_path)?;

    let import_versions: HashMap<String, Vec<i128>> = imports
        .iter()
        .map(|(alias, schema)| {
            let mut versions: Vec<i128> = schema.versions.iter().map(|v| v.version).collect();
            versions.sort_unstable();
            (alias.clone(), versions)
        })
        .collect();
    store.import_versions.insert(uri.clone(), import_versions);

    let mut ir = SchemaAnalyzer::new(ast, imports);
    ir.run()?;
    ir.finish()?;
    Ok(())
}

fn handle_text_update(
    store: &mut DocStore,
    uri: Uri,
    text: String,
    connection: &Connection,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let own_path = uri_to_path(&uri);

    let diagnostics = match parse_ast(&text) {
        Ok(ast) => {
            let analysis_result = match &own_path {
                Some(p) => analyze(&ast, p, store, &uri),
                None if ast.imports.is_empty() => Ok(()),
                None => Err(SchemaError::Analysis(AnalysisError::ImportNotFound {
                    path: "<unsaved document>".to_string(),
                    span: ast.span,
                    line: ast.line,
                })),
            };
            store.last_good_ast.insert(uri.clone(), ast);
            match analysis_result {
                Ok(_) => Vec::new(),
                Err(err) => {
                    let line_index = LineIndex::new(&text);
                    vec![error_to_diagnostic(&err, &text, &line_index)]
                }
            }
        }
        Err(err) => {
            let line_index = LineIndex::new(&text);
            vec![error_to_diagnostic(&err, &text, &line_index)]
        }
    };
    store.set(uri.clone(), text);
    publish_diagnostics(connection, uri, diagnostics)
}

fn position_to_offset(text: &str, pos: Position) -> usize {
    let mut offset = 0usize;
    let mut lines = text.split('\n');
    for _ in 0..pos.line {
        match lines.next() {
            Some(line) => offset += line.len() + 1,
            None => return text.len(),
        }
    }
    let line = lines.next().unwrap_or("");
    let mut utf16_count = 0u32;
    let mut byte_offset = 0usize;
    for c in line.chars() {
        if utf16_count >= pos.character {
            break;
        }
        utf16_count += c.len_utf16() as u32;
        byte_offset += c.len_utf8();
    }
    offset + byte_offset
}

fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    url::Url::parse(uri.as_str()).ok()?.to_file_path().ok()
}
