use std::path::Path;

use anyhow::Result;

use crate::symbol::Symbol;
use crate::tree::{Tree, filter_tree_by_path, tree_from_symbols};

pub struct PrintOptions {
    pub depth: u32,
    pub filter: Option<String>,
    pub include_mangled: bool,
    pub show_metadata: bool,
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
                println!("{prefix}└── ({total_symbols} symbols)");
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
                            println!("{prefix}{item_prefix} {name} ({count} symbols)");

                            // Only recurse if we haven't reached the max depth
                            if max_depth > 1 {
                                print_tree(
                                    child,
                                    &format!("{prefix}{child_prefix}"),
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


pub fn print_symbols(
    binary_path: &Path,
    symbols: Vec<Symbol>,
    options: PrintOptions,
) -> Result<()> {
    let depth = options.depth;

    if depth == 0 {
        let total_symbols = symbols.len();
        println!("{}", binary_path.display());
        println!("└── {total_symbols} total symbols");
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
            display_path = format!("{display_path}/{filter_path}");
        } else {
            println!("{}/{}", binary_path.display(), filter_path);
            println!("└── (no symbols found)");
            return Ok(());
        }
    }

    // Print the file path as root
    println!("{display_path}");

    // Print the entire tree using the single print_tree function
    print_tree(&tree, "", depth, &options);

    Ok(())
}

