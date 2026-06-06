use std::process::Command;
use std::fs;
use std::path::Path;

fn main() {
    // protobuf
    prost_build::compile_protos(&["schemas/player.proto"], &["schemas/"])
        .expect("failed to compile protos");

    // capnproto
    capnpc::CompilerCommand::new()
        .file("schemas/player.capnp")
        .output_path(std::env::var("OUT_DIR").unwrap())
        .run()
        .expect("failed to compile capnp schema");
    
    // flatbuffers
    let status = Command::new("flatc")
        .args(["-r", "-o", "src/generated/", "schemas/player.fbs"])
        .status()
        .expect("failed to run flatc");

    if !status.success() {
        panic!("flatc failed");
    }

    let dst = "src/generated/flatbuf.rs";

    if Path::new(dst).exists() {
        fs::remove_file(dst).expect("failed to remove existing file");
    }

    fs::rename(
        "src/generated/player_generated.rs",
        dst
    ).expect("rename failed");

    println!("cargo:rerun-if-changed=schemas/player.fbs");
    println!("cargo:rerun-if-changed=schemas/player.capnp");
    println!("cargo:rerun-if-changed=schemas/player.proto");
}