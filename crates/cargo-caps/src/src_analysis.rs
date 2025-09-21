use std::{collections::BTreeSet, fs};

use proc_macro2::Span;
use syn::{Type, UseTree, punctuated::Punctuated, spanned::Spanned as _, visit::Visit};

use crate::rust_path::RustPath;

#[derive(Debug)]
pub struct Import {
    /// This is short for…
    pub ident: String,

    /// …all of this.
    pub path: RustPath,
}

/// Represents all external dependencies found in code
#[derive(Debug, Default)]
pub struct ExternalUsage {
    /// Was there anything unrecognized/usupported in the code?
    ///
    /// This parser is not complete, and when we encounter something we don't support,
    /// we put it in this bucket.
    /// Anything in here is suspicious, and so if this is non-empty, then
    /// we can't trust the source code.
    pub unsupported: Vec<Span>,

    /// All
    pub all_paths: BTreeSet<RustPath>,

    /// e.g. `use std::fs;`
    pub imports: Vec<Import>,
}

impl ExternalUsage {
    /// Visit use statements and extract external uses
    fn visit_use_tree(&mut self, prefix: RustPath, use_tree: &UseTree) {
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

impl<'ast> Visit<'ast> for ExternalUsage {
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

/// Main parser for analyzing Rust external dependencies
pub struct RustExternalParser;

impl RustExternalParser {
    pub fn new() -> Self {
        Self
    }

    /// Parse a Rust source file and return external usage information
    pub fn parse_file<P: AsRef<std::path::Path>>(
        &self,
        rs_file_path: P,
    ) -> Result<ExternalUsage, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(rs_file_path)?;
        self.parse_content(&content)
    }

    /// Parse Rust source code content and return external usage information
    pub fn parse_content(
        &self,
        rust_source: &str,
    ) -> Result<ExternalUsage, Box<dyn std::error::Error>> {
        let syntax_tree = syn::parse_file(rust_source)?;
        let mut usage = ExternalUsage::default();
        usage.visit_file(&syntax_tree);
        Ok(usage)
    }
}

impl Default for RustExternalParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {

    use itertools::Itertools as _;

    use super::*;

    fn as_vec(all_paths: &BTreeSet<RustPath>) -> Vec<String> {
        all_paths.iter().map(|p| p.to_string()).collect()
    }

    fn format_unsupported(usage: &ExternalUsage) -> String {
        usage
            .unsupported
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

    #[test]
    fn test_external_crates() {
        let parser = RustExternalParser::new();
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

        let usage = parser.parse_content(content).unwrap();

        assert!(
            usage.unsupported.is_empty(),
            "Source code contained unsupported syntax at {}",
            format_unsupported(&usage)
        );

        assert_eq!(
            as_vec(&usage.all_paths),
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
