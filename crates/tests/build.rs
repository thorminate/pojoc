use bebop_tools as bebop;
use pojoc_build::codegen::generate;
use pojoc_build::schema::ImportOrchestrator;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // pojoc
    println!("cargo:rerun-if-changed=schemas/");

    let mut orchestrator = ImportOrchestrator::new();

    for entry in fs::read_dir("schemas").expect("schemas/ not found") {
        let path = entry.unwrap().path();

        if path.extension().and_then(|e| e.to_str()) != Some("pojoc") {
            continue;
        }

        let stem = path.file_stem().unwrap().to_str().unwrap().to_owned();

        let ir = orchestrator
            .resolve_root(&path)
            .unwrap_or_else(|e| panic!("failed to compile {}\n{e}", path.display()));

        let code = generate(&ir);
        fs::write(out_dir.join(format!("pojoc_{stem}.rs")), code)
            .unwrap_or_else(|e| panic!("failed to write {stem}.rs\n{e}"));
    }

    // protobuf
    prost_build::compile_protos(
        &["schemas/player.proto", "schemas/intern_bench.proto"],
        &["schemas/"],
    )
    .expect("failed to compile protos");

    // capnproto
    capnpc::CompilerCommand::new()
        .file("schemas/player.capnp")
        .file("schemas/intern_bench.capnp")
        .output_path(&out_dir)
        .run()
        .expect("failed to compile capnp schema");

    // flatbuffers
    for fbs in ["player.fbs", "intern_bench.fbs"] {
        let status = Command::new("flatc")
            .args([
                "-r",
                "-o",
                out_dir.to_str().unwrap(),
                &format!("schemas/{fbs}"),
            ])
            .status()
            .unwrap_or_else(|e| panic!("failed to run flatc on {fbs}\n{e}"));

        if !status.success() {
            panic!("flatc failed on {fbs}");
        }
    }

    fs::rename(
        out_dir.join("player_generated.rs"),
        out_dir.join("flatbuf.rs"),
    )
    .expect("rename failed");
    fs::rename(
        out_dir.join("intern_bench_generated.rs"),
        out_dir.join("flatbuf_intern_bench.rs"),
    )
    .expect("rename failed");

    // bebop
    bebop::download_bebopc(out_dir.join("bebop"));

    bebop::build_schema_dir(
        "schemas",
        out_dir.join("bebop-schema"),
        &bebop::BuildConfig {
            format_files: true,
            generate_module_file: false,
            skip_generated_notice: false,
        },
    );

    // bebopc emits `#![allow(warnings)]` as an inner attribute that isn't
    // the first item once spliced via `include!` into `generated!`'s own
    // module body — strip it from every generated file, not just player's.
    for name in ["player", "intern_bench"] {
        let bebop_out = out_dir.join(format!("bebop-schema/{name}.rs"));
        let src = fs::read_to_string(&bebop_out).unwrap();
        fs::write(&bebop_out, src.replace("#![allow(warnings)]", "")).unwrap();
    }

    println!("cargo:rerun-if-changed=schemas/player.fbs");
    println!("cargo:rerun-if-changed=schemas/player.capnp");
    println!("cargo:rerun-if-changed=schemas/player.proto");
    println!("cargo:rerun-if-changed=schemas/intern_bench.fbs");
    println!("cargo:rerun-if-changed=schemas/intern_bench.capnp");
    println!("cargo:rerun-if-changed=schemas/intern_bench.proto");
}
