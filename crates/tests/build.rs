use pojoc_codegen::generate;
use pojoc_schema::{ir::analyzer::*, lexer::*, parser::*};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // pojoc
    println!("cargo:rerun-if-changed=schemas/");

    for entry in fs::read_dir("schemas").expect("schemas/ not found") {
        let path = entry.unwrap().path();

        if path.extension().and_then(|e| e.to_str()) != Some("pojoc") {
            continue;
        }

        let stem = path.file_stem().unwrap().to_str().unwrap().to_owned();

        let raw = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}\n{e}", path.display()));
        let source = raw.strip_prefix('\u{feff}').unwrap_or(&raw);

        let tokens = Lexer::new(source)
            .tokenize()
            .unwrap_or_else(|e| panic!("failed to tokenize {}\n{e}", path.display()));
        let ast = Parser::new(tokens)
            .parse_schema()
            .unwrap_or_else(|e| panic!("failed to parse {}\n{e}", path.display()));

        let mut ir = SchemaAnalyzer::new(&ast);
        ir.run()
            .unwrap_or_else(|e| panic!("failed to analyze {}\n{e}", path.display()));
        let ir = ir
            .finish()
            .unwrap_or_else(|e| panic!("failed to finish analysis for {}\n{e}", path.display()));

        let code = generate(&ir);
        fs::write(out_dir.join(format!("pojoc_{stem}.rs")), code)
            .unwrap_or_else(|e| panic!("failed to write {stem}.rs\n{e}"));
    }

    // protobuf
    prost_build::compile_protos(&["schemas/player.proto"], &["schemas/"])
        .expect("failed to compile protos");

    // capnproto
    capnpc::CompilerCommand::new()
        .file("schemas/player.capnp")
        .output_path(&out_dir)
        .run()
        .expect("failed to compile capnp schema");

    // flatbuffers
    let status = Command::new("flatc")
        .args(["-r", "-o", out_dir.to_str().unwrap(), "schemas/player.fbs"])
        .status()
        .expect("failed to run flatc");

    if !status.success() {
        panic!("flatc failed");
    }

    fs::rename(
        out_dir.join("player_generated.rs"),
        out_dir.join("flatbuf.rs"),
    )
    .expect("rename failed");

    println!("cargo:rerun-if-changed=schemas/player.fbs");
    println!("cargo:rerun-if-changed=schemas/player.capnp");
    println!("cargo:rerun-if-changed=schemas/player.proto");
}
