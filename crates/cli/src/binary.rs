use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::process::Command;

pub fn build(
    out_dir: &Path,
    stem: &OsStr,
    codec: &str,
    no_serde: bool,
    runtime_path: Option<&Path>,
) -> io::Result<()> {
    let runtime = runtime_dependency(runtime_path)?;
    let out_dir = out_dir.canonicalize()?;
    let project = tempfile::tempdir_in(&out_dir)?;
    let path = project.path();
    let src = path.join("src");
    std::fs::create_dir(&src)?;
    std::fs::write(src.join("codec.rs"), codec)?;
    std::fs::write(
        src.join("main.rs"),
        concat!(
            "pub mod codec { include!(\"codec.rs\"); }\n",
            include_str!("sidecar.rs.in"),
        ),
    )?;

    let serde = if no_serde {
        ""
    } else {
        "serde = { version = \"1\", features = [\"derive\"] }\n"
    };
    std::fs::write(
        path.join("Cargo.toml"),
        format!(
            "[package]\nname = \"pojoc-sidecar\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
             [workspace]\n\
             [dependencies]\npojoc = {{ {runtime}, default-features = {} }}\n{serde}",
            !no_serde,
        ),
    )?;

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(path)
        .args([
            "install",
            "--path",
            ".",
            "--root",
            "install",
            "--no-track",
            "--quiet",
        ])
        .status()?;
    if !status.success() {
        return Err(io::Error::other(format!("Cargo exited with {status}")));
    }

    let suffix = std::env::consts::EXE_SUFFIX;
    let executable = path
        .join("install")
        .join("bin")
        .join(format!("pojoc-sidecar{suffix}"));
    let mut output_name = stem.to_os_string();
    output_name.push(suffix);
    std::fs::rename(executable, out_dir.join(output_name))
}

fn runtime_dependency(runtime_path: Option<&Path>) -> io::Result<String> {
    // A compiler built from a checkout can generate references to runtime APIs
    // that have not reached crates.io yet, even at the same package version.
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("../runtime");
    let path = runtime_path.or_else(|| {
        checkout
            .join("Cargo.toml")
            .is_file()
            .then_some(checkout.as_path())
    });
    let version = env!("CARGO_PKG_VERSION");
    let mut dependency = format!("version = \"={version}\"");
    if let Some(path) = path {
        let path = path.canonicalize().map_err(|err| {
            io::Error::new(
                err.kind(),
                format!("runtime path `{}`: {err}", path.display()),
            )
        })?;
        if !path.join("Cargo.toml").is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("missing Cargo.toml in runtime path `{}`", path.display()),
            ));
        }
        let path = path.to_str().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "runtime path must be UTF-8")
        })?;
        let path = serde_json::to_string(path).map_err(io::Error::other)?;
        dependency.push_str(&format!(", path = {path}"));
    }
    Ok(dependency)
}
