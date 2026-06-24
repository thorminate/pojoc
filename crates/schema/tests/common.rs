use pojoc_schema::ast::*;
use pojoc_schema::error::*;
use pojoc_schema::ir::analyzer::*;
use pojoc_schema::ir::ir_types::*;
use pojoc_schema::lexer::*;
use pojoc_schema::parser::*;
use std::collections::HashMap;

pub fn parse_schema(input: &str) -> Result<SchemaAst, SchemaError> {
    let tokens = Lexer::new(input).tokenize()?;
    Ok(Parser::new(tokens).parse_schema()?)
}

pub fn analyze_schema(ast: &SchemaAst) -> Result<ResolvedSchema, SchemaError> {
    let mut analyzer = SchemaAnalyzer::new(ast, HashMap::new());
    analyzer.run()?;
    Ok(analyzer.finish()?)
}
