mod completion;

use lsp_server::*;
use lsp_types::*;
use std::collections::HashMap;
use std::error::Error;
use pojoc_schema::{Position as SchemaPosition, Lexer, LexError, Parser, ParseError, AnalysisError, SchemaError, LineIndex, Span, SchemaAst};
use pojoc_schema::analyzer::SchemaAnalyzer;
use crate::completion::{completions_for_position, SchemaIndex};

struct DocStore {
    docs: HashMap<Uri, String>,
    last_good_ast: HashMap<Uri, SchemaAst>,
}

impl DocStore {
    fn new() -> Self {
        Self { docs: HashMap::new(), last_good_ast: HashMap::new() }
    }
    fn set(&mut self, uri: Uri, text: String) {
        self.docs.insert(uri, text);
    }
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let (connection, io_threads) = Connection::stdio();

    let server_capabilities = serde_json::to_value(ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::FULL,
        )),
        completion_provider: Some(CompletionOptions::default()),
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
                        let params: CompletionParams = serde_json::from_value(req.params.clone()).ok()?;
                        let uri = &params.text_document_position.text_document.uri;
                        let pos = params.text_document_position.position;
                        let text = store.docs.get(uri)?;
                        let offset = position_to_offset(text, pos);
                        let idx = store.last_good_ast.get(uri).map(SchemaIndex::build).unwrap_or_default();
                        Some(completions_for_position(text, offset, &idx))
                    })().unwrap_or_default();

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
                                if let Err(e) = handle_text_update(&mut store, uri, change.text, &connection) {
                                    eprintln!("failed to handle didChange: {e}");
                                }
                            }
                        }
                        Err(e) => eprintln!("malformed didChange params: {e}"),
                    }
                }
                "textDocument/didClose" => {
                    if let Ok(params) = serde_json::from_value::<DidCloseTextDocumentParams>(notif.params) {
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

fn publish_diagnostics(connection: &Connection, uri: Uri, diagnostics: Vec<Diagnostic>)
                       -> Result<(), Box<dyn Error + Send + Sync>>
{
    let params = PublishDiagnosticsParams { uri, diagnostics, version: None };
    connection.sender.send(Message::Notification(Notification::new(
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
        SchemaError::Lex(LexError::UnexpectedChar { span, .. }) => *span,
        SchemaError::Parse(e) => extract_parse_span(e, text),
        SchemaError::Analysis(e) => extract_analysis_span(e),
    }
}

fn extract_parse_span(err: &ParseError, text: &str) -> Span {
    match err {
        ParseError::UnexpectedToken { span, .. } => *span,
        ParseError::InvalidSyntax { span, .. } => *span,
        ParseError::UnexpectedEof => {
            let end = text.len();
            Span::new(end, end)
        }
    }
}

fn extract_analysis_span(err: &AnalysisError) -> Span {
    use AnalysisError::*;
    match err {
        UnknownType { span, .. }
        | UnknownParentType { span, .. }
        | ExtendsWithFullDefinition { span, .. }
        | FieldNotFound { span, .. }
        | MissingDefault { span, .. }
        | FieldAlreadyExists { span, .. }
        | FixedStringDefaultLengthMismatch { span, .. }
        | FixedSizeTooLarge { span, .. }
        | TypeMismatch { span, .. }
        | VarintsCannotBeConst { span, .. }
        | InvalidVFloat { span, .. }
        | VFloatRangeTooLarge { span, .. }
        | VFloatDefaultOutOfRange { span, .. }
        | InvalidDeltaElementType { span, .. }
        | ReservedVariantName { span, .. }
        | LazyDiffFieldMustBeOptional { span, .. }
        | NoVersions { span, .. } => *span,
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
    Parser::new(tokens).parse_schema().map_err(SchemaError::from)
}

fn analyze(ast: &SchemaAst) -> Result<(), SchemaError> {
    let mut ir = SchemaAnalyzer::new(ast);
    ir.run()?;
    ir.finish()?;
    Ok(())
}

fn handle_text_update(store: &mut DocStore, uri: Uri, text: String, connection: &Connection)
                      -> Result<(), Box<dyn Error + Send + Sync>>
{
    let diagnostics = match parse_ast(&text) {
        Ok(ast) => {
            let analysis_result = analyze(&ast);
            store.last_good_ast.insert(uri.clone(), ast); // AST cached even if analysis fails
            match analysis_result {
                Ok(_) => Vec::new(),
                Err(err) => {
                    let line_index = LineIndex::new(&text);
                    vec![error_to_diagnostic(&err, &text, &line_index)]
                }
            }
        }
        Err(err) => {
            // parse failed entirely — leave last_good_ast untouched, completion
            // falls back to whatever AST last parsed cleanly.
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
        if utf16_count >= pos.character { break; }
        utf16_count += c.len_utf16() as u32;
        byte_offset += c.len_utf8();
    }
    offset + byte_offset
}