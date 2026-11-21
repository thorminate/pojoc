use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::process::Command;

pub fn build(out_dir: &Path, stem: &OsStr, codec: &str, no_serde: bool) -> io::Result<()> {
    let project = tempfile::tempdir_in(out_dir.canonicalize()?)?;
    let path = project.path();
    let src = path.join("src");
    std::fs::create_dir(&src)?;
    std::fs::write(src.join("codec.rs"), codec)?;
    std::fs::write(src.join("main.rs"), include_str!("sidecar.rs.in"))?;

    let version = env!("CARGO_PKG_VERSION");
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
             [dependencies]\npojoc = {{ version = \"={version}\", default-features = {} }}\n{serde}",
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
