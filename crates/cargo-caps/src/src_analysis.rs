//! The point of this crate is to parse _simple_ `build.rs` files
//! and extract all [`RustPath`]s used in them.
//!
//! This is because the normal symbol-extraction we use for libraries
//! does not (yet) work for executables, because they tend to pull in
//! _a lot_ of unrelated symbols (TODO: consider calling strip on them???)
//!
//! TODO: make sure this things fails _safe_, i.e. that unreqcognized syntax
//! leads to an error rather than to assumling the build.rs is safe.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
};

use anyhow::Context as _;
use cargo_metadata::camino::{Utf8Path, Utf8PathBuf};
use itertools::Itertools as _;
use proc_macro2::Span;
use syn::{Type, UseTree, punctuated::Punctuated, spanned::Spanned as _, visit::Visit};

use crate::{
    capability::{Capability, Reason, Reasons},
    rust_path::RustPath,
};

pub struct ParsedRust {
    /// All full (absolute) paths we detected.
    pub all_paths: BTreeSet<RustPath>,

    /// All capabilities we detected.
    pub capabilities: BTreeMap<Capability, Reasons>,
}

impl ParsedRust {
    /// Parse a Rust source file and return external usage information
    pub fn parse_file<P: AsRef<Utf8Path>>(path: P) -> anyhow::Result<Self> {
        let path = path.as_ref();
        log::debug!("Parsing {path}");

        let mut all_paths = BTreeSet::new();
        let mut all_capabilities: BTreeMap<Capability, Reasons> = BTreeMap::new();
        let mut file_queue = VecDeque::new();
        let mut processed_files = BTreeSet::new();

        file_queue.push_back(path.to_path_buf());

        while let Some(current_file) = file_queue.pop_front() {
            if processed_files.contains(&current_file) {
                continue;
            }
            processed_files.insert(current_file.clone());

            log::debug!("Processing file: {current_file}");
            let content =
                fs::read_to_string(&current_file).with_context(|| current_file.to_string())?;

            let ParserState {
                all_paths: file_paths,
                unsupported,
                module_queue,
                capabilities,
                imports: _, // Used up
                current_file: _,
            } = ParserState::parse_content(&content, &current_file)?;

            if !unsupported.is_empty() {
                anyhow::bail!(
                    "Source code contained syntax that cargo-caps is too dumb to understand: {}",
                    format_unsupported(&unsupported)
                );
            }

            all_paths.extend(file_paths);
            for (capability, reasons) in capabilities {
                all_capabilities
                    .entry(capability)
                    .or_default()
                    .extend(reasons);
            }
            file_queue.extend(module_queue);
        }

        Ok(Self {
            all_paths,
            capabilities: all_capabilities,
        })
    }

    /// Parse Rust source code content and return external usage information
    fn parse_content(rust_source: &str) -> anyhow::Result<Self> {
        // This method is kept for backward compatibility but now uses the current directory
        let current_dir = Utf8PathBuf::from(".");
        let ParserState {
            all_paths,
            unsupported,
            module_queue: _,
            capabilities,
            imports: _, // Used up
            current_file: _,
        } = ParserState::parse_content(rust_source, &current_dir)?;

        if !unsupported.is_empty() {
            anyhow::bail!(
                "Source code contained syntax that cargo-caps is too dumb to understand: {}",
                format_unsupported(&unsupported)
            )
        }

        Ok(Self {
            all_paths,
            capabilities,
        })
    }
}

fn format_unsupported(unsupported: &[Span]) -> String {
    unsupported
        .iter()
        .map(|span| {
            let start = span.start();
            if let Some(src) = span.source_text() {
                format!("{}:{}: {src}", start.line, start.column)
            } else {
                format!("{}:{}", start.line, start.column)
            }
        })
        .join(", ")
}

#[derive(Debug)]
struct Import {
    /// This is short for…
    ident: String,

    /// …all of this.
    path: RustPath,
}

/// Represents all external dependencies found in code
#[derive(Debug, Default)]
struct ParserState {
    /// All full (absolute) paths we detected.
    all_paths: BTreeSet<RustPath>,

    /// Was there anything unrecognized/usupported in the code?
    ///
    /// This parser is not complete, and when we encounter something we don't support,
    /// we put it in this bucket.
    /// Anything in here is suspicious, and so if this is non-empty, then
    /// we can't trust the source code.
    unsupported: Vec<Span>,

    /// Queue of module files to be processed.
    module_queue: Vec<Utf8PathBuf>,

    /// Capabilities we detected in the code.
    capabilities: BTreeMap<Capability, Reasons>,

    /// Imports. Used during parsing.
    imports: Vec<Import>,

    /// Current file being parsed (used for resolving module paths).
    current_file: Utf8PathBuf,
}

impl ParserState {
    /// Parse Rust source code content and return external usage information
    fn parse_content(rust_source: &str, current_file: &Utf8Path) -> anyhow::Result<Self> {
        let syntax_tree = syn::parse_file(rust_source)?;
        let mut usage = Self {
            current_file: current_file.to_path_buf(),
            ..Self::default()
        };
        usage.visit_file(&syntax_tree);

        Ok(usage)
    }

    /// Visit use statements and extract external uses
    fn visit_use_tree(&mut self, prefix: RustPath, use_tree: &UseTree) {
        // TODO: push/pop scopes - do not assume all `use`s are at the top of the file.
        match use_tree {
            UseTree::Path(use_path) => {
                self.visit_use_tree(
                    prefix.with_segment(use_path.ident.to_string()),
                    &use_path.tree,
                );
            }
            UseTree::Name(use_name) => {
                let ident = use_name.ident.to_string();

                if ident == "self" {
                    self.imports.push(Import {
                        ident: (*prefix.segments().last().unwrap()).to_owned(),
                        path: prefix,
                    });
                } else {
                    let full_path = prefix.with_segment(ident.clone());
                    self.imports.push(Import {
                        ident,
                        path: full_path,
                    });
                }
            }
            UseTree::Rename(use_rename) => {
                let full_path = prefix.with_segment(use_rename.ident.to_string());
                // self.all_paths.insert(full_path.clone());
                self.imports.push(Import {
                    ident: use_rename.rename.to_string(),
                    path: full_path,
                });
            }
            UseTree::Glob(_) => {
                self.all_paths.insert(prefix);
            }
            UseTree::Group(use_group) => {
                for tree in &use_group.items {
                    self.visit_use_tree(prefix.clone(), tree);
                }
            }
        }
    }

    /// Process any external path found
    fn process_path(&mut self, syn_path: &syn::Path) {
        let rust_path = as_rust_path(syn_path);
        let segments = rust_path.segments();

        // TODO: resolve all paths _last_ because use statements can come _after_ usages:
        //
        //    fs::read(…);
        //    use std::fs;
        if let Some(import) = self
            .imports
            .iter()
            .find(|import| import.ident == segments[0])
        {
            // We have a `use X as Y` matching a `Y::…`:
            let full_path = RustPath::from_segments(
                import
                    .path
                    .segments()
                    .iter()
                    .chain(segments.iter().skip(1))
                    .map(|&s| s.to_owned()),
            );
            self.all_paths.insert(full_path);
        } else if segments.len() < 2 {
            // Skip single identifiers that doesn't match an import.
            // They are probably referring to locals.
        } else {
            // Assume this is already a fully qualified path
            self.all_paths.insert(rust_path);
        }
    }
}

fn as_rust_path(syn_path: &syn::Path) -> RustPath {
    RustPath::from_segments(syn_path.segments.iter().map(|seg| seg.ident.to_string()))
}

impl<'ast> Visit<'ast> for ParserState {
    fn visit_item_mod(&mut self, item_mod: &'ast syn::ItemMod) {
        if item_mod.content.is_none() {
            // This is an external module declaration (e.g., `mod foo;`)
            let module_name = item_mod.ident.to_string();

            let current_dir = self
                .current_file
                .parent()
                .unwrap_or_else(|| Utf8Path::new("."));

            let candidates = [
                current_dir.join(format!("{module_name}.rs")),
                current_dir.join(&module_name).join("mod.rs"),
            ];

            for candidate in candidates {
                if candidate.exists() {
                    self.module_queue.push(candidate);
                }
            }
        }

        // Continue visiting nested items
        syn::visit::visit_item_mod(self, item_mod);
    }

    fn visit_item_fn(&mut self, item_fn: &'ast syn::ItemFn) {
        // Check if function is unsafe
        if item_fn.sig.unsafety.is_some() {
            self.capabilities
                .entry(Capability::Unsafe)
                .or_default()
                .insert(Reason::SourceCodeAnalysis);
        }
        // Continue visiting nested items
        syn::visit::visit_item_fn(self, item_fn);
    }

    fn visit_expr_unsafe(&mut self, expr_unsafe: &'ast syn::ExprUnsafe) {
        // This handles unsafe expressions like `unsafe { ... }`
        self.capabilities
            .entry(Capability::Unsafe)
            .or_default()
            .insert(Reason::SourceCodeAnalysis);
        // Continue visiting nested items
        syn::visit::visit_expr_unsafe(self, expr_unsafe);
    }

    /// Visit use items (use statements)
    fn visit_item_use(&mut self, item_use: &'ast syn::ItemUse) {
        self.visit_use_tree(RustPath::new(""), &item_use.tree);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        self.process_path(path);

        // Continue visiting nested expressions
        syn::visit::visit_path(self, path);
    }

    /// Visit types to find external types
    fn visit_type(&mut self, ty: &'ast Type) {
        match ty {
            Type::Path(type_path) => {
                if let Some(qself) = &type_path.qself {
                    // Handle qualified types like <T as std::fmt::Display>::Output
                    syn::visit::visit_type(self, &qself.ty);
                }
                self.process_path(&type_path.path);
            }

            Type::Array(_)
            | Type::BareFn(_)
            | Type::Group(_)
            | Type::ImplTrait(_)
            | Type::Infer(_)
            | Type::Never(_)
            | Type::Paren(_)
            | Type::Ptr(_)
            | Type::Reference(_)
            | Type::Slice(_)
            | Type::TraitObject(_)
            | Type::Tuple(_) => {
                // the recursion should handle these cases
            }

            Type::Macro(_) | Type::Verbatim(_) | _ => {
                self.unsupported.push(ty.span());
            }
        }

        // Continue visiting nested types
        syn::visit::visit_type(self, ty);
    }

    fn visit_attribute(&mut self, input: &'ast syn::Attribute) {
        #[expect(clippy::match_same_arms)]
        match &input.meta {
            syn::Meta::Path(_) => {
                // e.g. `#[test]`
            }
            syn::Meta::List(meta_list) => {
                // e.g. `#[derive(…)]
                // meta_list.path == "derive"

                // Parse the comma-separated list of derive traits
                if let Ok(derive_list) = syn::parse2::<DeriveList>(meta_list.tokens.clone()) {
                    for syn_path in derive_list.paths {
                        self.process_path(&syn_path);
                    }
                }
            }
            syn::Meta::NameValue(_) => {
                // A name-value pair within an attribute, like `feature = "nightly"`.
            }
        }

        // Continue visiting nested items
        syn::visit::visit_attribute(self, input);
    }
}

struct DeriveList {
    paths: Punctuated<syn::Path, syn::Token![,]>,
}

impl syn::parse::Parse for DeriveList {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        Ok(Self {
            paths: input.parse_terminated(syn::Path::parse, syn::Token![,])?,
        })
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    fn as_vec(all_paths: &BTreeSet<RustPath>) -> Vec<String> {
        all_paths.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn test_external_crates() {
        let content = r#"
            use serde::{self, Deserialize};
            use tokio::runtime::Runtime;
            use std::fs::self;
            use std::net as netty;

            fn main() {
                let path = todo!();
                let data = std::fs::read(path);
                let json = serde_json::to_string(&data).unwrap();
                let runtime = Runtime::new().unwrap();
                tokio::pin!(async_operation);
                ::log::info!("Starting application");
                fs::File::open();
                netty::TcpServer::new();
            }

            #[derive(serde::Serialize, Deserialize)]
            struct Foo {}
        "#;

        let all_paths = ParsedRust::parse_content(content).unwrap().all_paths;

        assert_eq!(
            as_vec(&all_paths),
            [
                "log::info",
                "serde::Deserialize",
                "serde::Serialize",
                "serde_json::to_string",
                "std::fs::File::open",
                "std::fs::read",
                "std::net::TcpServer::new",
                "tokio::pin",
                "tokio::runtime::Runtime::new",
            ]
        );
    }

    #[test]
    fn test_unsafe_detection() {
        let content_with_unsafe_fn = r#"
            unsafe fn dangerous_function() {
                // This is an unsafe function
            }
        "#;

        let content_with_unsafe_block = r#"
            fn main() {
                let raw_ptr = 0x123 as *const i32;
                let value = unsafe { *raw_ptr };
            }
        "#;

        let content_without_unsafe = r#"
            fn safe_function() {
                println!("This is safe");
            }
        "#;

        // Test unsafe function
        let result = ParsedRust::parse_content(content_with_unsafe_fn).unwrap();
        assert!(
            result.capabilities.contains_key(&Capability::Unsafe),
            "Should detect unsafe function"
        );

        // Test unsafe block
        let result = ParsedRust::parse_content(content_with_unsafe_block).unwrap();
        assert!(
            result.capabilities.contains_key(&Capability::Unsafe),
            "Should detect unsafe block"
        );

        // Test safe code
        let result = ParsedRust::parse_content(content_without_unsafe).unwrap();
        assert!(
            !result.capabilities.contains_key(&Capability::Unsafe),
            "Should not detect unsafe in safe code"
        );
    }
}
