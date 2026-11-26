use std::path::PathBuf;

use pojoc_compiler::{codegen, core::types::set_emit_serde, schema::ImportOrchestrator};
use proc_macro::TokenStream;
use quote::quote;
use syn::{
    Ident, LitStr, Token, Visibility,
    parse::{Parse, ParseStream},
    parse_macro_input,
};

mod kw {
    syn::custom_keyword!(from);
    syn::custom_keyword!(serde);
}

struct SchemaInput {
    module: Option<(Visibility, Ident)>,
    path: LitStr,
    serde: bool,
}

impl Parse for SchemaInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let module = if input.peek(LitStr) {
            None
        } else {
            let vis = input.parse()?;
            input.parse::<Token![mod]>()?;
            let name = input.parse()?;
            input.parse::<kw::from>()?;
            Some((vis, name))
        };
        let path = input.parse()?;
        let serde = if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            input.parse::<kw::serde>()?;
            true
        } else {
            false
        };
        Ok(Self {
            module,
            path,
            serde,
        })
    }
}

/// Compiles a `.pojoc` schema into Rust items during compilation.
///
/// Relative paths start at the directory containing the calling package's
/// `Cargo.toml`. Cargo tracks changes to the schema and its transitive imports.
/// The calling crate must depend on the `pojoc` runtime.
///
/// ```ignore
/// pojoc_build::schema!(pub mod player from "schemas/player.pojoc");
/// ```
///
/// Use `schema!("schemas/player.pojoc")` to emit the items in the current module.
/// Add `, serde` to generate Serde implementations. That form requires `serde`
/// with its `derive` feature and the `pojoc` runtime with its `serde` feature.
#[proc_macro]
pub fn schema(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as SchemaInput);
    expand(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand(input: SchemaInput) -> syn::Result<proc_macro2::TokenStream> {
    let span = input.path.span();
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .ok_or_else(|| syn::Error::new(span, "CARGO_MANIFEST_DIR is unavailable"))?;
    let path = PathBuf::from(manifest_dir).join(input.path.value());

    let mut orchestrator = ImportOrchestrator::new();
    let resolved = orchestrator
        .resolve_root(&path)
        .map_err(|error| syn::Error::new(span, error.render(&path)))?;

    let mut sources: Vec<_> = orchestrator.source_paths().collect();
    sources.sort_unstable();
    let sources = sources
        .into_iter()
        .map(|path| {
            let path_text = path.to_str().ok_or_else(|| {
                syn::Error::new(
                    span,
                    format!("schema path is not UTF-8: {}", path.display()),
                )
            })?;
            Ok(LitStr::new(path_text, span))
        })
        .collect::<syn::Result<Vec<_>>>()?;

    let previous_serde = set_emit_serde(input.serde);
    let code = codegen::generate(&resolved);
    set_emit_serde(previous_serde);
    let items = code.parse::<proc_macro2::TokenStream>().map_err(|error| {
        syn::Error::new(span, format!("could not tokenize generated Rust: {error}"))
    })?;

    // Record filesystem reads in rustc's dep-info so Cargo invalidates the expansion.
    let body = quote! {
        #(const _: &[u8] = ::core::include_bytes!(#sources);)*
        #items
    };
    Ok(match input.module {
        Some((vis, name)) => quote! { #vis mod #name { #body } },
        None => body,
    })
}
