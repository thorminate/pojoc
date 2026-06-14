use std::str::from_utf8;
use std::{fs, path::PathBuf, process::Command};

pub struct BuildOptions {
    pub project_name: String,
    pub target: Option<String>,
    pub release: bool,
    pub runtime_path: PathBuf,
}

pub fn build_project(
    generated_code: &str,
    opts: &BuildOptions,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let project_name_lower = opts.project_name.to_lowercase();

    let build_dir = std::env::temp_dir().join(format!("pojoc_build_{}", project_name_lower));
    let src_dir = build_dir.join("src");

    if build_dir.exists() {
        fs::remove_dir_all(&build_dir)?;
    }

    fs::create_dir_all(&src_dir)?;

    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
pojoc = {{ path = "{runtime_path}" }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
"#,
        name = project_name_lower,
        runtime_path = opts.runtime_path.display()
    );

    fs::write(build_dir.join("Cargo.toml"), cargo_toml)?;

    let main_rs = format!(
        r#"
use std::{{env, io::Write}};

mod generated;
use serde_json;

fn main() {{
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {{
        eprintln!("Usage:");
        eprintln!("  decode <input.bin> [output.json]");
        eprintln!("  encode <input.json> [output.bin]");
        std::process::exit(1);
    }}

    let cmd = &args[1];
    let input = &args[2];
    let output = args.get(3).map(|x| x.as_str());
    
    match cmd.as_str() {{
        "decode" => decode(input, output),
        "encode" => encode(input, output),
        _ => {{
            eprintln!("Unknown command: {{}}", cmd);
            std::process::exit(1);
        }}
    }}
}}

fn decode(input: &str, output: Option<&str>) {{
    let data = std::fs::read(input).expect("failed to read input file");

    let value = generated::decode(&data).expect("decode failed");

    let json = serde_json::to_string_pretty(&value)
        .unwrap_or_else(|_| format!("{{:#?}}", value));

    match output {{
        Some(path) => std::fs::write(path, json).expect("write failed"),
        None => println!("{{}}", json),
    }}
}}

fn encode(input: &str, output: Option<&str>) {{
    let json = std::fs::read_to_string(input).expect("failed to read input file");

    let value: generated::{project_name} =
        serde_json::from_str(&json).expect("invalid json input");

    let mut buf = Vec::new();
    generated::encode(&mut buf, &value);

    match output {{
        Some(path) => std::fs::write(path, buf).expect("write failed"),
        None => {{
            std::io::stdout().write_all(&buf).unwrap();
            std::io::stdout().flush().unwrap();
        }}
    }}
}}
"#,
        project_name = opts.project_name
    );

    fs::write(src_dir.join("main.rs"), main_rs)?;
    fs::write(src_dir.join("generated.rs"), generated_code)?;

    let cache_dir = std::env::temp_dir().join("pojoc_cache");

    let mut cmd = Command::new("cargo");

    cmd.env("CARGO_TARGET_DIR", &cache_dir);
    cmd.arg("build");

    if opts.release {
        cmd.arg("--release");
    }

    if let Some(target) = &opts.target {
        cmd.arg("--target").arg(target);
    }

    cmd.current_dir(&build_dir);

    let out = cmd.output()?;
    if !out.status.success() {
        return Err(format!(
            "cargo build failed: {}\n{}",
            out.status,
            from_utf8(out.stderr.as_slice()).expect("could not ascertain stderr from cargo build")
        )
        .into());
    }

    let profile = if opts.release { "release" } else { "debug" };

    let bin_name = if cfg!(windows) {
        format!("{}.exe", project_name_lower)
    } else {
        opts.project_name.clone()
    };

    let output = if let Some(target) = &opts.target {
        cache_dir.join(target).join(profile).join(bin_name)
    } else {
        cache_dir.join(profile).join(bin_name)
    };

    Ok(output)
}
