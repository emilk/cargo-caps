use proc_macro2::Span;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use syn::{Path as SynPath, Type, UseTree, spanned::Spanned as _, visit::Visit};

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

    /// Direct use statements (e.g., use std::fs::File, use serde::Serialize)
    pub use_statements: Vec<String>,
    /// Qualified paths in expressions (e.g., std::fs::File::open(), serde_json::from_str())
    pub qualified_paths: HashSet<String>,
    /// All external modules/crates referenced
    pub modules: HashSet<String>,
    /// Types from external crates
    pub types: HashSet<String>,
    /// Root crates used (e.g., std, serde, tokio)
    pub root_crates: HashSet<String>,
}

impl ExternalUsage {
    /// Merge another ExternalUsage into this one
    pub fn merge(&mut self, other: ExternalUsage) {
        self.use_statements.extend(other.use_statements);
        self.qualified_paths.extend(other.qualified_paths);
        self.modules.extend(other.modules);
        self.types.extend(other.types);
        self.root_crates.extend(other.root_crates);
    }

    /// Get a summary of external usage
    pub fn summary(&self) -> String {
        let mut summary = String::new();
        summary.push_str("External Dependencies Summary:\n");
        summary.push_str(&format!("- Root crates: {}\n", self.root_crates.len()));
        summary.push_str(&format!(
            "- Use statements: {}\n",
            self.use_statements.len()
        ));
        summary.push_str(&format!(
            "- Qualified paths: {}\n",
            self.qualified_paths.len()
        ));
        summary.push_str(&format!("- Unique modules: {}\n", self.modules.len()));
        summary.push_str(&format!("- Types: {}\n", self.types.len()));

        if !self.root_crates.is_empty() {
            summary.push_str("\nRoot crates:\n");
            let mut sorted: Vec<_> = self.root_crates.iter().collect();
            sorted.sort();
            for crate_name in sorted {
                summary.push_str(&format!("  - {}\n", crate_name));
            }
        }

        if !self.use_statements.is_empty() {
            summary.push_str("\nUse statements:\n");
            for stmt in &self.use_statements {
                summary.push_str(&format!("  - {}\n", stmt));
            }
        }

        if !self.qualified_paths.is_empty() {
            summary.push_str("\nQualified paths:\n");
            let mut sorted: Vec<_> = self.qualified_paths.iter().collect();
            sorted.sort();
            for path in sorted {
                summary.push_str(&format!("  - {}\n", path));
            }
        }

        if !self.modules.is_empty() {
            summary.push_str("\nModules:\n");
            let mut sorted: Vec<_> = self.modules.iter().collect();
            sorted.sort();
            for module in sorted {
                summary.push_str(&format!("  - {}\n", module));
            }
        }

        summary
    }

    /// Check if a specific crate is used
    pub fn uses_crate(&self, crate_name: &str) -> bool {
        self.root_crates.contains(crate_name)
    }

    /// Check if a specific module is used
    pub fn uses_module(&self, module: &str) -> bool {
        self.modules.contains(module)
            || self
                .modules
                .iter()
                .any(|m| m.starts_with(&format!("{module}::")))
            || self
                .qualified_paths
                .iter()
                .any(|p| p.starts_with(&format!("{module}::")))
    }

    /// Get all modules from a specific crate
    pub fn modules_from_crate(&self, crate_name: &str) -> Vec<&String> {
        self.modules
            .iter()
            .filter(|module| {
                module.starts_with(&format!("{crate_name}::")) || module == &crate_name
            })
            .collect()
    }

    /// Check if any std library is used
    pub fn uses_std(&self) -> bool {
        self.uses_crate("std") || self.uses_crate("core") || self.uses_crate("alloc")
    }

    /// Get all non-std crates used
    pub fn external_crates(&self) -> Vec<&String> {
        self.root_crates
            .iter()
            .filter(|crate_name| {
                !matches!(
                    crate_name.as_str(),
                    "std" | "core" | "alloc" | "self" | "super" | "crate"
                )
            })
            .collect()
    }
}

/// Visitor that walks the AST and collects external dependencies
struct ExternalVisitor {
    usage: ExternalUsage,
}

impl ExternalVisitor {
    fn new() -> Self {
        Self {
            usage: ExternalUsage::default(),
        }
    }

    /// Convert a syn::Path to a string
    fn path_to_string(&self, path: &SynPath) -> String {
        path.segments
            .iter()
            .map(|seg| seg.ident.to_string())
            .collect::<Vec<_>>()
            .join("::")
    }

    /// Extract root crate and module paths from a full path
    fn extract_path_info(&self, path_str: &str) -> (Option<String>, Vec<String>) {
        let parts: Vec<&str> = path_str.split("::").collect();
        if parts.is_empty() {
            return (None, vec![]);
        }

        let root_crate = parts[0].to_owned();
        let mut modules = Vec::new();

        // Add the root crate itself as a module
        modules.push(root_crate.clone());

        // Add increasingly specific module paths
        for i in 1..parts.len() {
            let module = parts[..=i].join("::");
            modules.push(module);
        }

        (Some(root_crate), modules)
    }

    /// Visit use statements and extract external uses
    fn visit_use_tree(&mut self, use_tree: &UseTree, prefix: &str) {
        match use_tree {
            UseTree::Path(use_path) => {
                let new_prefix = if prefix.is_empty() {
                    use_path.ident.to_string()
                } else {
                    format!("{}::{}", prefix, use_path.ident)
                };
                self.visit_use_tree(&use_path.tree, &new_prefix);
            }
            UseTree::Name(use_name) => {
                let full_path = if prefix.is_empty() {
                    use_name.ident.to_string()
                } else {
                    format!("{}::{}", prefix, use_name.ident)
                };

                // Only record if it looks like an external path
                if full_path.contains("::")
                    || matches!(full_path.as_str(), "std" | "core" | "alloc")
                    || full_path.chars().next().is_some_and(|c| c.is_lowercase())
                {
                    self.usage.use_statements.push(full_path.clone());
                    let (root_crate, modules) = self.extract_path_info(&full_path);
                    if let Some(crate_name) = root_crate {
                        self.usage.root_crates.insert(crate_name);
                    }
                    for module in modules {
                        self.usage.modules.insert(module);
                    }
                }
            }
            UseTree::Rename(use_rename) => {
                let full_path = if prefix.is_empty() {
                    use_rename.ident.to_string()
                } else {
                    format!("{}::{}", prefix, use_rename.ident)
                };

                if full_path.contains("::")
                    || matches!(full_path.as_str(), "std" | "core" | "alloc")
                    || full_path.chars().next().is_some_and(|c| c.is_lowercase())
                {
                    self.usage
                        .use_statements
                        .push(format!("{} as {}", full_path, use_rename.rename));
                    let (root_crate, modules) = self.extract_path_info(&full_path);
                    if let Some(crate_name) = root_crate {
                        self.usage.root_crates.insert(crate_name);
                    }
                    for module in modules {
                        self.usage.modules.insert(module);
                    }
                }
            }
            UseTree::Glob(_) => {
                if !prefix.is_empty()
                    && (prefix.contains("::")
                        || matches!(prefix, "std" | "core" | "alloc")
                        || prefix.chars().next().is_some_and(|c| c.is_lowercase()))
                {
                    self.usage.use_statements.push(format!("{prefix}::*"));
                    let (root_crate, modules) = self.extract_path_info(prefix);
                    if let Some(crate_name) = root_crate {
                        self.usage.root_crates.insert(crate_name);
                    }
                    for module in modules {
                        self.usage.modules.insert(module);
                    }
                }
            }
            UseTree::Group(use_group) => {
                for tree in &use_group.items {
                    self.visit_use_tree(tree, prefix);
                }
            }
        }
    }

    /// Process any external path found
    fn process_path(&mut self, path: &SynPath) {
        let path_str = self.path_to_string(path);

        // Skip single identifiers that are likely local variables/functions
        if !path_str.contains("::") {
            return;
        }

        self.usage.qualified_paths.insert(path_str.clone());
        let (root_crate, modules) = self.extract_path_info(&path_str);
        if let Some(crate_name) = root_crate {
            self.usage.root_crates.insert(crate_name);
        }
        for module in modules {
            self.usage.modules.insert(module);
        }

        self.usage.types.insert(path_str);
    }
}

impl<'ast> Visit<'ast> for ExternalVisitor {
    /// Visit use items (use statements)
    fn visit_item_use(&mut self, item_use: &'ast syn::ItemUse) {
        self.visit_use_tree(&item_use.tree, "");
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
            Type::Verbatim(..) => {
                self.usage.unsupported.push(ty.span());
            }
            _ => {
                self.usage.unsupported.push(ty.span());
            }
        }

        // Continue visiting nested types
        syn::visit::visit_type(self, ty);
    }
}

/// Main parser for analyzing Rust external dependencies
pub struct RustExternalParser;

impl RustExternalParser {
    pub fn new() -> Self {
        Self
    }

    /// Parse a Rust source file and return external usage information
    pub fn parse_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<ExternalUsage, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        self.parse_content(&content)
    }

    /// Parse Rust source code content and return external usage information
    pub fn parse_content(
        &self,
        content: &str,
    ) -> Result<ExternalUsage, Box<dyn std::error::Error>> {
        let syntax_tree = syn::parse_file(content)?;
        let mut visitor = ExternalVisitor::new();
        visitor.visit_file(&syntax_tree);
        Ok(visitor.usage)
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

    #[test]
    fn test_std_usage() {
        let parser = RustExternalParser::new();
        let content = r#"
            fn main() {
                let file = std::fs::File::open("test.txt").unwrap();
                let mut map = std::collections::HashMap::new();
            }
        "#;

        let usage = parser.parse_content(content).unwrap();
        assert!(usage.uses_crate("std"));
        assert!(usage.uses_module("std::fs"));
        assert!(usage.uses_module("std::collections"));
        assert!(usage.qualified_paths.contains("std::fs::File::open"));
        assert!(
            usage
                .qualified_paths
                .contains("std::collections::HashMap::new")
        );
        assert!(
            usage.unsupported.is_empty(),
            "Source code contained unsupported syntax at {}",
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
        );
    }

    #[test]
    fn test_external_crates() {
        let parser = RustExternalParser::new();
        let content = r#"
            use serde::{Serialize, Deserialize};
            use tokio::runtime::Runtime;

            fn main() {
                let json = serde_json::to_string(&data).unwrap();
                let runtime = tokio::runtime::Runtime::new().unwrap();
            }
        "#;

        let usage = parser.parse_content(content).unwrap();
        assert!(usage.uses_crate("serde"));
        assert!(usage.uses_crate("tokio"));
        assert!(usage.uses_crate("serde_json"));

        let external_crates = usage.external_crates();
        assert!(external_crates.contains(&&"serde".to_owned()));
        assert!(external_crates.contains(&&"tokio".to_owned()));
        assert!(external_crates.contains(&&"serde_json".to_owned()));
    }

    #[test]
    fn test_mixed_usage() {
        let parser = RustExternalParser::new();
        let content = r#"
            use std::collections::HashMap;
            use serde::Serialize;

            fn main() {
                let mut map = HashMap::new();
                let file = std::fs::File::open("test.txt").unwrap();
                let json = serde_json::to_string(&data).unwrap();
                println!("Hello");
            }
        "#;

        let usage = parser.parse_content(content).unwrap();
        assert!(usage.uses_crate("std"));
        assert!(usage.uses_crate("serde"));
        assert!(usage.uses_crate("serde_json"));
        assert!(usage.uses_module("std::collections"));
        assert!(usage.uses_module("std::fs"));
    }

    #[test]
    fn test_type_usage() {
        let parser = RustExternalParser::new();
        let content = r#"
            fn process_data(data: serde_json::Value) -> std::io::Result<tokio::sync::mpsc::Receiver<String>> {
                todo!()
            }
        "#;

        let usage = parser.parse_content(content).unwrap();
        assert!(usage.uses_crate("serde_json"));
        assert!(usage.uses_crate("std"));
        assert!(usage.uses_crate("tokio"));
        assert!(usage.types.contains("serde_json::Value"));
        assert!(usage.types.contains("std::io::Result"));
        assert!(usage.types.contains("tokio::sync::mpsc::Receiver"));
    }

    #[test]
    fn test_macro_usage() {
        let parser = RustExternalParser::new();
        let content = r#"
            fn main() {
                tokio::pin!(async_operation);
                log::info!("Starting application");
            }
        "#;

        let usage = parser.parse_content(content).unwrap();
        assert!(usage.uses_crate("tokio"));
        assert!(usage.uses_crate("log"));
    }
}
