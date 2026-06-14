use pojoc_codegen::builder::{BuildOptions, build_project};
use pojoc_codegen::generate;
use pojoc_schema::ir::analyzer::SchemaAnalyzer;
use pojoc_schema::lexer::Lexer;
use pojoc_schema::parser::Parser;

use std::path::PathBuf;

fn main() {
    let src = include_str!("player.pojoc");
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);

    let tokens = Lexer::new(src).tokenize().expect("lex error");

    let ast = Parser::new(tokens).parse_schema().expect("parse error");

    let mut ir = SchemaAnalyzer::new(&ast);
    ir.run().expect("ir could not compile");
    let ir = ir.finish().expect("ir could not finish");

    let generated = generate(&ir);

    let runtime_path = PathBuf::from("C:/dev/Rust/pojoc/crates/runtime");

    let opts = BuildOptions {
        project_name: ast.name.clone(),
        target: None,
        release: true,
        runtime_path,
    };

    let binary_path = build_project(&generated, &opts).unwrap_or_else(|e| {
        eprintln!("failed to build temp cargo project:\n{}", e);
        std::process::exit(1);
    });

    println!("Built binary at: {}", binary_path.display());
}
