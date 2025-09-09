use std::{collections::BTreeMap, path::Path};

use anyhow::Result;

use crate::symbol::{Symbol, SymbolCategory};

pub struct PrintOptions {
    pub depth: u32,
}

#[derive(Debug, Clone)]
pub enum Tree {
    Leaf(Symbol),
    Node(BTreeMap<String, Tree>),
}

impl Tree {
    /// Create a Tree from a list of symbols, grouping by dot-separated prefixes
    pub fn from_symbols(symbols: &[Symbol]) -> Self {
        let grouped = group_symbols_by_prefix(symbols);
        Self::Node(grouped)
    }

    /// Get the count of symbols in this tree
    pub fn symbol_count(&self) -> usize {
        match self {
            Tree::Leaf(_) => 1,
            Tree::Node(children) => children.values().map(|child| child.symbol_count()).sum(),
        }
    }
}

fn group_symbols_by_prefix(symbols: &[Symbol]) -> BTreeMap<String, Tree> {
    let mut grouped: BTreeMap<String, Vec<Symbol>> = BTreeMap::new();

    for symbol in symbols {
        let name = &symbol.demangled;

        // Special handling for GCC_except_table symbols
        let prefix = if name.starts_with("GCC_except_table") {
            "GCC_except_table"
        } else if let Some(dot_pos) = name.find('.') {
            // Extract prefix before first dot, or use the entire name if no dot
            &name[..dot_pos]
        } else {
            name
        };

        grouped
            .entry(prefix.to_string())
            .or_default()
            .push(symbol.clone());
    }

    // Convert grouped symbols to Tree nodes
    grouped
        .into_iter()
        .map(|(prefix, symbols)| {
            let tree = if symbols.len() == 1 {
                Tree::Leaf(symbols.into_iter().next().unwrap())
            } else {
                Tree::Node(
                    symbols
                        .into_iter()
                        .map(|symbol| (symbol.demangled.clone(), Tree::Leaf(symbol)))
                        .collect(),
                )
            };
            (prefix, tree)
        })
        .collect()
}

/// Print a tree structure with proper indentation and tree characters
fn print_tree(tree: &Tree, prefix: &str, max_depth: u32) {
    match tree {
        Tree::Leaf(symbol) => {
            // TODO: add option to print mangled name
            println!("{}└── {}", prefix, symbol.demangled);
        }
        Tree::Node(children) => {
            if max_depth > 0 {
                let child_entries: Vec<_> = children.iter().collect();
                for (i, (name, child)) in child_entries.iter().enumerate() {
                    let is_last = i == child_entries.len() - 1;
                    let item_prefix = if is_last { "└──" } else { "├──" };
                    let child_prefix = if is_last { "    " } else { "│   " };

                    match child {
                        Tree::Leaf(symbol) => {
                            // TODO: add option to print mangled name
                            println!("{}{} {}", prefix, item_prefix, symbol.demangled);
                        }
                        Tree::Node(_) => {
                            let count = child.symbol_count();
                            println!("{}{} {} ({} symbols)", prefix, item_prefix, name, count);

                            // Only recurse if we haven't reached the max depth
                            if max_depth > 1 {
                                print_tree(
                                    child,
                                    &format!("{}{}", prefix, child_prefix),
                                    max_depth - 1,
                                );
                            }
                        }
                    }
                }
            } else {
                // Show count only when max depth is reached
                let total_symbols = tree.symbol_count();
                println!("{}└── ({} symbols)", prefix, total_symbols);
            }
        }
    }
}

fn get_or_create_category<'a>(
    root: &'a mut BTreeMap<String, Tree>,
    name: impl Into<String>,
) -> &'a mut BTreeMap<String, Tree> {
    let category = root
        .entry(name.into())
        .or_insert_with(|| Tree::Node(BTreeMap::new()));
    if let Tree::Node(children) = category {
        children
    } else {
        unreachable!("Category should always be a Node")
    }
}

fn tree_from_symbols(symbols: &[Symbol]) -> Tree {
    let mut root = BTreeMap::new();

    for symbol in symbols {
        match &symbol.category {
            SymbolCategory::Crate(_) => {
                let category = get_or_create_category(&mut root, "crates");
                tree_from_symbol(category, symbol);
            }
            SymbolCategory::TraitImpl {
                trait_for: _,
                target_crate,
            } => {
                let category = get_or_create_category(&mut root, "trait_impls");
                let subcategory = get_or_create_category(category, target_crate);
                tree_from_symbol(subcategory, symbol);
            }
            SymbolCategory::Compiler(_) => {
                let category = get_or_create_category(&mut root, "compiler");
                tree_from_symbol(category, symbol);
            }
            SymbolCategory::System(_) => {
                let category = get_or_create_category(&mut root, "system");
                let name = &symbol.demangled;
                let system_category = if name.starts_with("GCC_except_table") {
                    "GCC_except_table"
                } else if let Some(dot_pos) = name.find('.') {
                    // Extract prefix before first dot, or use the entire name if no dot
                    &name[..dot_pos]
                } else {
                    name
                };
                let sub_category = get_or_create_category(category, system_category);
                tree_from_symbol(sub_category, symbol);
            }
            SymbolCategory::Unknown => {
                let category = get_or_create_category(&mut root, "unknown");
                tree_from_symbol(category, symbol);
            }
        }
    }

    Tree::Node(root)
}

pub fn print_symbols(
    binary_path: &Path,
    symbols: Vec<Symbol>,
    options: PrintOptions,
) -> Result<()> {
    let depth = options.depth;

    if depth == 0 {
        let total_symbols = symbols.len();
        println!("{}", binary_path.display());
        println!("└── {} total symbols", total_symbols);
        return Ok(());
    }

    let tree = tree_from_symbols(&symbols);

    // Print the file path as root
    println!("{}", binary_path.display());

    // Print the entire tree using the single print_tree function
    print_tree(&tree, "", depth);

    Ok(())
}

fn tree_from_symbol(root: &mut BTreeMap<String, Tree>, symbol: &Symbol) {
    // Split by :: to create hierarchical structure
    let parts: Vec<&str> = symbol.demangled.split("::").collect();
    insert_symbol_into_tree(root, &parts, symbol.clone());
}

// Recursively insert a symbol into the tree based on its path parts
fn insert_symbol_into_tree(
    current_node: &mut BTreeMap<String, Tree>,
    parts: &[&str],
    symbol: Symbol,
) {
    if parts.is_empty() {
        return;
    }

    if parts.len() == 1 {
        // This is a leaf node - use the demangled symbol name as key
        current_node.insert(symbol.demangled.clone(), Tree::Leaf(symbol));
    } else {
        // This is an intermediate node - recurse deeper
        let current_part = parts[0].to_string();
        let remaining_parts = &parts[1..];

        // Get or create the child node
        let child_node = current_node
            .entry(current_part)
            .or_insert_with(|| Tree::Node(BTreeMap::new()));

        // Recurse into the child node
        if let Tree::Node(child_map) = child_node {
            insert_symbol_into_tree(child_map, remaining_parts, symbol);
        }
    }
}
