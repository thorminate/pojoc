use clap::{Args, Parser, Subcommand, ValueEnum};
use pojoc_build::codegen::generate;
use pojoc_build::codegen::typescript::{GenerationOptions, RuntimeProfile, generate_with_options};
use pojoc_build::core::types::TypeId;
use pojoc_build::schema::ir::ir_types::{FieldIR, ResolvedConst, ResolvedSchema, latest_by_name};
use pojoc_build::schema::render::{
    render_default_value, render_modifiers, render_type_ref, rust_type_to_schema,
};
use pojoc_build::schema::{AnalysisError, ImportOrchestrator};
use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::Instant;

mod binary;

#[derive(Parser)]
#[command(name = "pojoc", version, about = "Pojoc schema compiler")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
enum Target {
    Rust,
    #[value(alias = "ts")]
    Typescript,
}

#[derive(Args)]
struct BuildArgs {
    /// Entry-point .pojoc file
    input: PathBuf,

    /// Output directory for generated source files
    #[arg(short, long, default_value = "out")]
    out_dir: PathBuf,

    /// Output target for the generated codec.
    #[arg(long, value_enum, default_value = "rust")]
    target: Target,

    /// Embed the runtime in the generated TypeScript module.
    #[arg(long)]
    standalone: bool,

    /// Omit dedicated TypeScript seek code for older wire layouts.
    #[arg(long)]
    compact: bool,

    /// Select a TypeScript runtime profile [portable, bun, node].
    #[arg(long, value_name = "PROFILE")]
    runtime: Option<RuntimeProfile>,

    /// Shorthand for --runtime bun.
    #[arg(long, conflicts_with = "runtime")]
    bun: bool,

    /// Print per-file diagnostics
    #[arg(short, long)]
    verbose: bool,

    /// Don't emit Serialize/Deserialize derives/impls in generated code.
    /// The consuming crate's `pojoc` dependency must have its own `serde`
    /// feature disabled to match, or the generated code won't compile.
    /// The standalone executable also omits its JSON stdio modes.
    #[arg(long)]
    no_serde: bool,

    /// Also compile a standalone, schema-specific executable using Cargo.
    #[arg(long = "bin")]
    emit_bin: bool,

    /// Use a specific Pojoc runtime crate for the standalone executable.
    #[arg(long, requires = "emit_bin", value_name = "DIR")]
    runtime_path: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Compile a schema and write output to disk
    Build(BuildArgs),

    /// Check a schema without writing output
    Check {
        /// Entry-point .pojoc file
        input: PathBuf,

        /// Print per-file diagnostics
        #[arg(short, long)]
        verbose: bool,
    },

    /// Print the schema's latest version with every diff applied
    Show {
        /// Entry-point .pojoc file
        input: PathBuf,

        /// Only print the root fields, omitting declared types
        #[arg(short, long)]
        fields_only: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    let code = match cli.command {
        Command::Build(args) => build(args),
        Command::Check { input, verbose } => check(input, verbose),
        Command::Show { input, fields_only } => show(input, fields_only),
    };

    std::process::exit(code);
}

fn render_error(err: &AnalysisError, root: &Path) {
    eprint!("{}", err.render(root));
}

/// warnings never fail the build, they just print alongside it. Includes
/// imported schemas', each rendered against its own file.
fn render_warnings(schema: &ResolvedSchema, root: &Path) {
    for (w, path) in schema.warnings_recursive(root) {
        eprint!("{}", w.render(path));
    }
}

fn log(verbose: bool, msg: &str) {
    if verbose {
        println!("\x1b[2m  {msg}\x1b[0m");
    }
}

fn build(args: BuildArgs) -> i32 {
    let BuildArgs {
        input,
        out_dir,
        target,
        standalone,
        compact,
        runtime,
        bun,
        verbose,
        no_serde,
        emit_bin,
        runtime_path,
    } = args;
    match target {
        Target::Rust if standalone => {
            eprintln!("error: --standalone is a TypeScript target option");
            return 1;
        }
        Target::Rust if compact => {
            eprintln!("error: --compact is a TypeScript target option");
            return 1;
        }
        Target::Rust if runtime.is_some() || bun => {
            eprintln!("error: --runtime and --bun are TypeScript target options");
            return 1;
        }
        Target::Typescript if emit_bin || no_serde => {
            eprintln!("error: --bin and --no-serde are Rust target options");
            return 1;
        }
        _ => {}
    }
    pojoc_build::set_emit_serde(!no_serde);

    let mut orchestrator = ImportOrchestrator::new();

    let t = Instant::now();
    let schema = match orchestrator.resolve_root(input.as_path()) {
        Ok(s) => {
            render_warnings(&s, &input);
            s
        }
        Err(e) => {
            render_error(&e, &input);
            return 1;
        }
    };
    log(
        verbose,
        &format!(
            "parsed & analyzed in {:.1}ms",
            t.elapsed().as_secs_f64() * 1000.0
        ),
    );

    let t = Instant::now();
    let (code, extension) = match target {
        Target::Rust => (generate(&schema), ".rs"),
        Target::Typescript => {
            let profile = if bun {
                RuntimeProfile::Bun
            } else {
                runtime.unwrap_or_default()
            };
            let result = generate_with_options(
                &schema,
                GenerationOptions {
                    standalone,
                    runtime: profile,
                    compact,
                },
            );
            match result {
                Ok(code) => (code, ".ts"),
                Err(err) => {
                    eprintln!("error: {err}");
                    return 1;
                }
            }
        }
    };
    log(
        verbose,
        &format!("codegen in {:.1}ms", t.elapsed().as_secs_f64() * 1000.0),
    );

    if !out_dir.exists()
        && let Err(e) = std::fs::create_dir_all(&out_dir)
    {
        eprintln!(
            "error: could not create output dir `{}`: {e}",
            out_dir.display()
        );
        return 1;
    }

    let stem = input.file_stem().unwrap();
    let mut source_name = stem.to_os_string();
    source_name.push(extension);
    let out_file = out_dir.join(source_name);

    let t = Instant::now();
    if let Err(e) = std::fs::write(&out_file, &code) {
        eprintln!("error: could not write `{}`: {e}", out_file.display());
        return 1;
    }
    log(
        verbose,
        &format!(
            "wrote `{}` in {:.1}ms",
            out_file.display(),
            t.elapsed().as_secs_f64() * 1000.0
        ),
    );

    if emit_bin
        && let Err(e) = binary::build(&out_dir, stem, &code, no_serde, runtime_path.as_deref())
    {
        eprintln!("error: could not build standalone executable: {e}");
        return 1;
    }

    0
}

fn check(input: PathBuf, verbose: bool) -> i32 {
    let mut orchestrator = ImportOrchestrator::new();

    let t = Instant::now();
    match orchestrator.resolve_root(input.as_path()) {
        Ok(s) => {
            render_warnings(&s, &input);
            log(
                verbose,
                &format!(
                    "check passed in {:.1}ms",
                    t.elapsed().as_secs_f64() * 1000.0
                ),
            );
            0
        }
        Err(e) => {
            render_error(&e, &input);
            1
        }
    }
}

/// ANSI styling for `show`, suppressed when stdout isn't a terminal or NO_COLOR
/// is set so piped output stays plain
#[derive(Clone, Copy)]
struct Style {
    on: bool,
}

impl Style {
    fn detect() -> Self {
        let on = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
        Self { on }
    }

    fn paint(&self, code: &str, s: &str) -> String {
        if self.on {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }

    /// `schema`, `type`, `fields`, ...
    fn keyword(&self, s: &str) -> String {
        self.paint("35", s)
    }
    /// a declared type/enum/union/bitset name
    fn decl(&self, s: &str) -> String {
        self.paint("1;33", s)
    }
    fn ty(&self, s: &str) -> String {
        self.paint("36", s)
    }
    /// `seekable`, `lazy`, `intern`, `delta` — the same set the TextMate grammar
    /// classifies as modifiers
    fn modifier(&self, s: &str) -> String {
        self.paint("32", s)
    }
    /// a literal: a default, an enum's wire value, a constraint bound
    fn value(&self, s: &str) -> String {
        self.paint("33", s)
    }
    fn dim(&self, s: &str) -> String {
        self.paint("2", s)
    }

    /// `= <literal>`, the one place a value is introduced
    fn assign(&self, literal: &str) -> String {
        format!("{} {}", self.dim("="), self.value(literal))
    }
}

/// colors an already-rendered type string, so `render_type_ref` stays free of
/// presentation concerns for its other caller (the LSP). Punctuation dims,
/// modifiers green, constraint bounds read as literals rather than type names,
/// and whitespace passes through untouched so the result is identical to the
/// plain rendering once escapes are stripped.
fn paint_type(s: Style, rendered: &str) -> String {
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '.' || c == '-';

    let mut out = String::new();
    let mut word = String::new();

    let flush = |word: &mut String, out: &mut String| {
        if word.is_empty() {
            return;
        }
        let painted = match word.as_str() {
            "intern" | "delta" => s.modifier(word),
            // parameter labels, not types
            "min" | "max" | "step" => s.dim(word),
            // a leading digit or sign is only ever a literal here: a bound, a
            // step, or a fixed-size count
            w if w.starts_with(|c: char| c.is_ascii_digit() || c == '-') => s.value(w),
            w => s.ty(w),
        };
        out.push_str(&painted);
        word.clear();
    };

    for c in rendered.chars() {
        if is_word(c) {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            if c.is_whitespace() {
                out.push(c);
            } else {
                out.push_str(&s.dim(&c.to_string()));
            }
        }
    }
    flush(&mut word, &mut out);
    out
}

/// each declared type at its latest version, name-sorted for stable output
fn latest_sorted<V>(map: &HashMap<TypeId, V>) -> Vec<(String, &V)> {
    let mut out: Vec<(String, &V)> = latest_by_name(map)
        .into_iter()
        .map(|(name, (_, v))| (name, v))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn show(input: PathBuf, fields_only: bool) -> i32 {
    let mut orchestrator = ImportOrchestrator::new();

    let schema = match orchestrator.resolve_root(input.as_path()) {
        Ok(s) => s,
        Err(e) => {
            render_error(&e, &input);
            return 1;
        }
    };
    render_warnings(&schema, &input);

    let Some(latest) = schema.versions.last() else {
        eprintln!("error: schema declares no versions");
        return 1;
    };

    let s = Style::detect();

    let count = schema.versions.len();
    let suffix = if count > 1 {
        s.dim(&format!(" (latest of {count})"))
    } else {
        String::new()
    };
    println!(
        "{} {} {}{suffix}",
        s.keyword("schema"),
        s.decl(&schema.name_hint),
        s.dim(&format!("v{}", latest.version)),
    );

    if !fields_only {
        for (name, e) in latest_sorted(&schema.enums.enums) {
            let body = e
                .variants
                .iter()
                .map(|v| format!("{} {}", v.name, s.assign(&v.wire_value.to_string())))
                .collect();
            print_block(s, "enum", Some(&name), body);
        }

        for (name, b) in latest_sorted(&schema.bitsets.bitsets) {
            let body = b.variants.iter().map(|v| v.name.clone()).collect();
            print_block(s, "bitset", Some(&name), body);
        }

        for (name, u) in latest_sorted(&schema.unions.unions) {
            let body = u
                .variants
                .iter()
                .map(|v| {
                    format!(
                        "{}: {}",
                        v.name,
                        paint_type(s, &render_type_ref(&v.payload))
                    )
                })
                .collect();
            print_block(s, "union", Some(&name), body);
        }

        for (name, t) in latest_sorted(&schema.types.types) {
            let body = t
                .const_fields
                .iter()
                .map(|c| const_line(s, c))
                .chain(t.fields.iter().map(|f| field_line(s, f)))
                .collect();
            print_block(s, "type", Some(&name), body);
        }
    }

    let body = latest
        .const_fields
        .iter()
        .map(|c| const_line(s, c))
        .chain(latest.fields.iter().map(|f| field_line(s, f)))
        .collect();
    print_block(s, "fields", None, body);

    0
}

/// a `keyword Name { .. }` block, collapsed to `{}` on one line when it has no
/// body. `name` is None for the root `fields` block, which is unnamed.
fn print_block(s: Style, keyword: &str, name: Option<&str>, body: Vec<String>) {
    let head = match name {
        Some(n) => format!("{} {}", s.keyword(keyword), s.decl(n)),
        None => s.keyword(keyword),
    };

    if body.is_empty() {
        println!("\n{head} {}", s.dim("{}"));
        return;
    }

    println!("\n{head} {}", s.dim("{"));
    for line in body {
        println!("  {line}");
    }
    println!("{}", s.dim("}"));
}

fn const_line(s: Style, c: &ResolvedConst) -> String {
    format!(
        "{}: {} {} {}",
        c.name,
        s.keyword("const"),
        paint_type(s, rust_type_to_schema(c.rust_type)),
        s.assign(&render_default_value(&c.value))
    )
}

/// `name: modifiers type = default`, with the default omitted when it has no
/// literal form (structs, maps, nested defaults)
fn field_line(s: Style, f: &FieldIR) -> String {
    let modifiers = render_modifiers(f);
    let mut out = format!("{}: ", f.name);
    if !modifiers.is_empty() {
        out.push_str(&s.modifier(modifiers.trim_end()));
        out.push(' ');
    }
    out.push_str(&paint_type(s, &render_type_ref(&f.ty)));

    match f.default.as_ref().map(render_default_value) {
        Some(d) if !d.is_empty() => {
            out.push(' ');
            out.push_str(&s.assign(&d));
        }
        _ => {}
    }
    out
}
