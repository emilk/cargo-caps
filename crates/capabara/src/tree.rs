use std::collections::BTreeMap;

use crate::{
    rust_path::RustPath,
    symbol::{Symbol, TraitFnImpl},
};

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
            Self::Leaf(_) => 1,
            Self::Node(children) => children.values().map(|child| child.symbol_count()).sum(),
        }
    }

    pub fn is_leaf(&self) -> bool {
        matches!(self, Self::Leaf(_))
    }

    /// Collapse nodes that contain only a single leaf, recursively
    pub fn collapse_single_nodes(self, depth: usize, may_collapse: bool) -> Self {
        match self {
            Self::Leaf(_) => self,
            Self::Node(mut children) => {
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
                    children.into_iter().next().expect("Child node should exist since we checked children.len() == 1").1
                } else {
                    Self::Node(children)
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
            .entry(prefix.to_owned())
            .or_default()
            .push(symbol.clone());
    }

    // Convert grouped symbols to Tree nodes
    grouped
        .into_iter()
        .map(|(prefix, symbols)| {
            let tree = if symbols.len() == 1 {
                Tree::Leaf(symbols.into_iter().next().expect("symbols vector should have exactly one element"))
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

pub fn get_or_create_category(
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

pub fn tree_from_symbols(symbols: &[Symbol]) -> Tree {
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
            }
            insert_leaf(category, &symbol);
        } else if let Ok(trait_impl) = TraitFnImpl::parse(demangled) {
            symbol.demangled = trait_impl.to_string();

            for path in trait_impl.paths() {
                let segments = path.segments();
                if segments.len() == 1 {
                    // Probably a built-in type, like [T]
                } else {
                    let crate_name = segments[0];
                    let category = crate_category(&mut root, crate_name);
                    insert_symbol_into_tree(category, &segments, symbol.clone());
                }
            }
        } else if demangled.contains("::") {
            let path = RustPath::new(demangled);
            let segments = path.segments();
            debug_assert!(segments.len() > 1, "Rust path should have more than one segment");
            let crate_name = segments[0];
            let category = crate_category(&mut root, crate_name);
            insert_symbol_into_tree(category, &segments, symbol.clone());
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
            insert_leaf(sub_category, &symbol);
        }
    }

    Tree::Node(root)
}

fn is_standard_crate(crate_name: &str) -> bool {
    ["alloc", "core", "std"].contains(&crate_name)
}

fn crate_category(
    root: &mut BTreeMap<String, Tree>,
    crate_name: impl Into<String>,
) -> &mut BTreeMap<String, Tree> {
    let crate_name = crate_name.into();
    if is_standard_crate(&crate_name) {
        root
    } else {
        get_or_create_category(root, "crates")
    }
}

pub fn filter_tree_by_path(tree: &Tree, path: &[&str]) -> Option<Tree> {
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
                    new_children.insert(first_segment.to_owned(), filtered_child);
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

fn insert_leaf(root: &mut BTreeMap<String, Tree>, symbol: &Symbol) {
    root.insert(symbol.demangled.clone(), Tree::Leaf(symbol.clone()));
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
        let current_part = parts[0].to_owned();
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
