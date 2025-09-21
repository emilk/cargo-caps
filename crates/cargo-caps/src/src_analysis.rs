//! The point of this crate is to parse _simple_ `build.rs` files
//! and extract all [`RustPath`]s used in them.
//!
//! This is because the normal symbol-extration we use for libraries
//! does not (yet) work for executables, because they tend to pull in
//! _a lot_ of unrelated symbols (TODO: consider calling strip on them???)
//!
//! TODO: make sure this things fails _safe_, i.e. that unreqcognized syntax
//! leads to an error rather than to assumling the build.rs is safe.

use std::{collections::BTreeSet, fs};

use anyhow::Context as _;
use cargo_metadata::camino::Utf8Path;
use itertools::Itertools as _;
use proc_macro2::Span;
use syn::{Type, UseTree, punctuated::Punctuated, spanned::Spanned as _, visit::Visit};

use crate::rust_path::RustPath;

pub struct ParsedRust {
    /// All full (absolute) paths we detected.
    pub all_paths: BTreeSet<RustPath>,
}

impl ParsedRust {
    /// Parse a Rust source file and return external usage information
    pub fn parse_file<P: AsRef<Utf8Path>>(path: P) -> anyhow::Result<Self> {
        let path = path.as_ref();
        log::debug!("Parsing {path}");
        let content = fs::read_to_string(path).with_context(|| path.to_string())?;
        Self::parse_content(&content).with_context(|| path.to_string())
    }

    /// Parse Rust source code content and return external usage information
    fn parse_content(rust_source: &str) -> anyhow::Result<Self> {
        let ParserState {
            all_paths,
            unsupported,
            has_external_mods,
            imports: _, // Used up
        } = ParserState::parse_content(rust_source)?;

        if has_external_mods {
            anyhow::bail!("cargo-caps doesn't support loading other module files");
        }

        if !unsupported.is_empty() {
            anyhow::bail!(
                "Source code contained syntax that cargo-caps is too dumb to understand: {}",
                format_unsupported(&unsupported)
            )
        }

        Ok(Self { all_paths })
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

    has_external_mods: bool,

    /// Imports. Used during parsing.
    imports: Vec<Import>,
}

impl ParserState {
    /// Parse Rust source code content and return external usage information
    fn parse_content(rust_source: &str) -> anyhow::Result<Self> {
        let syntax_tree = syn::parse_file(rust_source)?;
        let mut usage = Self::default();
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
            self.has_external_mods = true;
        }
        // Continue visiting nested items
        syn::visit::visit_item_mod(self, item_mod);
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
}
