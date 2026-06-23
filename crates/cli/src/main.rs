use pojoc_codegen::builder::{BuildOptions, build_project};
use pojoc_codegen::generate;
use pojoc_schema::ImportOrchestrator;

use std::path::PathBuf;

fn main() {
    // Was include_str! — fine for a single file, but import resolution
    // needs a real path to resolve relative `import "..."` declarations
    // against. Assumes player.pojoc sits next to this main.rs in src/;
    // adjust the path if your layout differs.
    let schema_path = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/src/player.pojoc"));

    let mut orchestrator = ImportOrchestrator::new();
    let ir = orchestrator.resolve_root(&schema_path).unwrap_or_else(|e| {
        eprintln!("schema compile failed: {e}");
        std::process::exit(1);
    });

    let generated = generate(&ir);

    let runtime_path = PathBuf::from("C:/dev/Rust/pojoc/crates/runtime");

    let opts = BuildOptions {
        project_name: ir.name_hint.clone(),
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
