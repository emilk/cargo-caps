use std::{collections::BTreeMap, path::Path};

use anyhow::Result;

use crate::symbol::{Symbol, TraitFnImpl};

pub struct PrintOptions {
    pub depth: u32,
    pub filter: Option<String>,
    pub include_mangled: bool,
    pub show_metadata: bool,
}

#[derive(Debug, Clone)]
pub enum Tree {
    Leaf(Symbol),
    Node(BTreeMap<String, Tree>),
}

impl Default for Tree {
    fn default() -> Self {
        Self::Node(BTreeMap::new())
    }
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

    pub fn is_leaf(&self) -> bool {
        matches!(self, Tree::Leaf(_))
    }

    /// Collapse nodes that contain only a single leaf, recursively
    pub fn collapse_single_nodes(self, depth: usize, may_collapse: bool) -> Self {
        match self {
            Tree::Leaf(_) => self,
            Tree::Node(mut children) => {
                // First, recursively collapse all children
                let may_collapse_child = children.len() == 1;
                for child in children.values_mut() {
                    *child =
                        std::mem::take(child).collapse_single_nodes(depth + 1, may_collapse_child);
                }

                if may_collapse
                    && 4 < depth
                    && children.len() == 1
                    && children.values().next().is_some_and(Self::is_leaf)
                {
                    children.into_iter().next().unwrap().1
                } else {
                    Tree::Node(children)
                }
            }
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
fn print_tree(tree: &Tree, prefix: &str, max_depth: u32, options: &PrintOptions) {
    match tree {
        Tree::Leaf(symbol) => {
            println!("{}└── {}", prefix, symbol.format_with_metadata(options));
        }
        Tree::Node(children) => {
            if max_depth == 0 {
                // Show count only when max depth is reached
                let total_symbols = tree.symbol_count();
                println!("{}└── ({} symbols)", prefix, total_symbols);
            } else {
                let child_entries: Vec<_> = children.iter().collect();
                for (i, (name, child)) in child_entries.iter().enumerate() {
                    let is_last = i == child_entries.len() - 1;
                    let item_prefix = if is_last { "└──" } else { "├──" };
                    let child_prefix = if is_last { "    " } else { "│   " };

                    match child {
                        Tree::Leaf(symbol) => {
                            println!(
                                "{}{} {}",
                                prefix,
                                item_prefix,
                                symbol.format_with_metadata(options)
                            );
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
                                    options,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

fn get_or_create_category(
    root: &mut BTreeMap<String, Tree>,
    name: impl Into<String>,
) -> &mut BTreeMap<String, Tree> {
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
        let mut symbol = symbol.clone();
        let demangled = &symbol.demangled;
        if demangled.starts_with("__rustc[") {
            let category = get_or_create_category(&mut root, "rustc");
            // Example: '__rustc[5224e6b81cd82a8f]::__rust_alloc'
            // Get part after `]::`:
            if let Some(end_bracket) = demangled.find("]::") {
                symbol.demangled = demangled[end_bracket + 3..].to_owned();
                tree_from_symbol(category, &symbol);
            } else {
                tree_from_symbol(category, &symbol);
            }
        } else if let Ok(trait_impl) = TraitFnImpl::parse(demangled) {
            symbol.demangled = trait_impl.to_string();

            // How do we categorize this?
            // This could be `impl ForeignTrait for LocalType`
            // or `impl LocalTrait for ForeignType`
            // or `impl LocalTrait for LocalType`.
            // The trait should always be namespaced to some crate,
            // but the type can be a built-in like `[T]` or `i32`.

            {
                let trait_parts: Vec<&str> = trait_impl.trait_name.split("::").collect();
                let crate_name = trait_parts[0];
                let category = crate_category(&mut root, crate_name);
                insert_symbol_into_tree(category, &trait_parts, symbol.clone());
            }

            {
                let type_parts: Vec<&str> = trait_impl.type_name.split("::").collect();
                if type_parts.len() == 1 {
                    // Probably a built-in type, like T
                } else {
                    let crate_name = type_parts[0];
                    let category = crate_category(&mut root, crate_name);
                    insert_symbol_into_tree(category, &type_parts, symbol.clone());
                }
            }
        } else if let Some(first_colon) = demangled.find("::") {
            let crate_name = &demangled[..first_colon];
            add_crate_symbol(&mut root, crate_name, &symbol);
        } else {
            let category = get_or_create_category(&mut root, "system");
            let name = &symbol.demangled;
            let system_category = if name.starts_with("GCC_except_table") {
                "GCC_except_table"
            } else if name.starts_with("lCPI") {
                // local Constant Pool Identifier
                "lCPI"
            } else if name.starts_with("ltmp") {
                "ltmp"
            } else if let Some(dot_pos) = name.find('.') {
                // Extract prefix before first dot, or use the entire name if no dot
                &name[..dot_pos]
            } else {
                name
            };
            let sub_category = get_or_create_category(category, system_category);
            tree_from_symbol(sub_category, &symbol);
        }
    }

    Tree::Node(root)
}

fn crate_category(
    root: &mut BTreeMap<String, Tree>,
    crate_name: impl Into<String>,
) -> &mut BTreeMap<String, Tree> {
    let crate_name = crate_name.into();
    if ["core", "std"].contains(&crate_name.as_str()) {
        root
    } else {
        get_or_create_category(root, "crates")
    }
}

fn add_crate_symbol(root: &mut BTreeMap<String, Tree>, crate_name: &str, symbol: &Symbol) {
    if ["core", "std"].contains(&crate_name) {
        tree_from_symbol(root, symbol);
    } else {
        let category = get_or_create_category(root, "crates");
        tree_from_symbol(category, symbol);
    }
}

fn filter_tree_by_path(tree: &Tree, path: &[&str]) -> Option<Tree> {
    if path.is_empty() {
        return Some(tree.clone());
    }

    match tree {
        Tree::Leaf(_) => None, // Can't navigate deeper from a leaf
        Tree::Node(children) => {
            let first_segment = path[0];
            let remaining_path = &path[1..];

            if let Some(child) = children.get(first_segment) {
                // If we found the matching child, recursively filter it
                if let Some(filtered_child) = filter_tree_by_path(child, remaining_path) {
                    // Create a new tree with only the path to the filtered content
                    let mut new_children = BTreeMap::new();
                    new_children.insert(first_segment.to_string(), filtered_child);
                    Some(Tree::Node(new_children))
                } else {
                    None
                }
            } else {
                None
            }
        }
    }
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

    let mut tree = tree_from_symbols(&symbols);
    if true {
        tree = tree.collapse_single_nodes(0, false);
    }
    let mut display_path = binary_path.display().to_string();

    // Apply filter if specified
    if let Some(filter_path) = &options.filter {
        let path_segments: Vec<&str> = filter_path.split('/').collect();
        if let Some(filtered_tree) = filter_tree_by_path(&tree, &path_segments) {
            tree = filtered_tree;
            display_path = format!("{}/{}", display_path, filter_path);
        } else {
            println!("{}/{}", binary_path.display(), filter_path);
            println!("└── (no symbols found)");
            return Ok(());
        }
    }

    // Print the file path as root
    println!("{}", display_path);

    // Print the entire tree using the single print_tree function
    print_tree(&tree, "", depth, &options);

    Ok(())
}

fn tree_from_symbol(root: &mut BTreeMap<String, Tree>, symbol: &Symbol) {
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
