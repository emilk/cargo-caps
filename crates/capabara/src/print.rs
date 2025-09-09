use std::{collections::BTreeMap, path::Path};

use anyhow::Result;

use crate::symbol::{Symbol, SymbolCategory, SystemSymbolType};

pub struct PrintOptions<'a> {
    pub depth: Option<u32>,
    pub filter_module: Option<&'a str>,
}

pub fn print_symbols(
    binary_path: &Path,
    symbols: Vec<Symbol>,
    options: PrintOptions,
) -> Result<()> {
    let mut symbols_by_category: BTreeMap<SymbolCategory, Vec<Symbol>> = BTreeMap::new();

    for sym in symbols {
        symbols_by_category
            .entry(sym.category.clone())
            .or_default()
            .push(sym);
    }

    print_symbols_by_category(binary_path, symbols_by_category, options)
}

fn print_symbols_by_category(
    binary_path: &Path,
    symbols_by_category: BTreeMap<SymbolCategory, Vec<Symbol>>,
    options: PrintOptions,
) -> Result<()> {
    // Filter to specific category if requested
    if let Some(filter_name) = options.filter_module {
        return print_filtered_category(binary_path, &symbols_by_category, filter_name, options.depth);
    }

    // Print hierarchical tree based on depth
    let total_symbols: usize = symbols_by_category.values().map(|v| v.len()).sum();
    let depth = options.depth.unwrap_or(1); // Default to depth 1 (show categories)
    
    println!("{}", binary_path.display());
    
    if depth == 0 {
        // Depth 0: Only summary
        println!("└── {} total symbols", total_symbols);
        return Ok(());
    }

    // Separate different types of categories
    let mut crates = Vec::new();
    let mut trait_impls_by_target: BTreeMap<String, Vec<(String, usize)>> = BTreeMap::new();
    let mut compiler = Vec::new();
    let mut system_by_type: BTreeMap<SystemSymbolType, Vec<Symbol>> = BTreeMap::new();
    let mut unknown = Vec::new();

    for (category, symbols) in &symbols_by_category {
        match category {
            SymbolCategory::Crate(name) => crates.push((name.clone(), symbols.clone())),
            SymbolCategory::TraitImpl { trait_for, target_crate } => {
                trait_impls_by_target
                    .entry(target_crate.clone())
                    .or_default()
                    .push((trait_for.clone(), symbols.len()));
            }
            SymbolCategory::Compiler(name) => compiler.push((name.clone(), symbols.clone())),
            SymbolCategory::System(_) => {
                // Group all system symbols together for later dot-prefix processing
                for symbol in symbols {
                    system_by_type
                        .entry(SystemSymbolType::Other("system".to_string()))
                        .or_default()
                        .push(symbol.clone());
                }
            }
            SymbolCategory::Unknown => unknown.extend(symbols.iter().cloned()),
        }
    }

    // Sort crates and compiler entries
    crates.sort_by(|a, b| a.0.cmp(&b.0));
    compiler.sort_by(|a, b| a.0.cmp(&b.0));

    let mut sections = Vec::new();
    
    // Add crates section
    if !crates.is_empty() {
        sections.push(("crates", crates.len()));
    }
    
    // Add trait implementations section
    if !trait_impls_by_target.is_empty() {
        let total_impls: usize = trait_impls_by_target.values().map(|v| v.len()).sum();
        sections.push(("trait_impls", total_impls));
    }
    
    // Add compiler section
    if !compiler.is_empty() {
        sections.push(("compiler", compiler.len()));
    }
    
    // Add system section
    if !system_by_type.is_empty() {
        // Count the total number of system symbols for display
        let total_system_symbols: usize = system_by_type.values().map(|v| v.len()).sum();
        sections.push(("system", total_system_symbols));
    }
    
    // Add unknown section
    if !unknown.is_empty() {
        sections.push(("unknown", 1));
    }

    // Print sections
    for (i, (section_name, section_count)) in sections.iter().enumerate() {
        let is_last_section = i == sections.len() - 1;
        let section_prefix = if is_last_section { "└──" } else { "├──" };
        let child_prefix = if is_last_section { "    " } else { "│   " };

        match *section_name {
            "crates" => {
                println!("{} crates ({} total)", section_prefix, section_count);
                if depth >= 2 {
                    print_crates_tree(&crates, child_prefix, depth);
                }
            }
            "trait_impls" => {
                println!("{} trait implementations ({} total)", section_prefix, section_count);
                if depth >= 2 {
                    print_trait_impls_tree(&trait_impls_by_target, child_prefix, depth);
                }
            }
            "compiler" => {
                println!("{} compiler ({} entries)", section_prefix, section_count);
                if depth >= 2 {
                    print_compiler_tree(&compiler, child_prefix, depth);
                }
            }
            "system" => {
                println!("{} system ({} symbols)", section_prefix, section_count);
                if depth >= 2 {
                    print_system_tree(&system_by_type, child_prefix, depth);
                }
            }
            "unknown" => {
                println!("{} unknown ({} symbols)", section_prefix, unknown.len());
                if depth >= 3 {
                    print_symbols_tree(&unknown, &format!("{}    ", child_prefix));
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn print_filtered_category(
    binary_path: &Path,
    symbols_by_category: &BTreeMap<SymbolCategory, Vec<Symbol>>,
    filter_name: &str,
    depth: Option<u32>,
) -> Result<()> {
    let mut found = false;
    for (category, symbols) in symbols_by_category {
        if category.to_string().contains(filter_name) {
            println!("{}", binary_path.display());
            println!("└── {} ({} symbols)", category, symbols.len());
            
            if depth.unwrap_or(2) >= 2 {
                print_symbols_tree(symbols, "    ");
            }
            
            found = true;
            break;
        }
    }

    if !found {
        println!("Category '{}' not found", filter_name);
        println!("Available categories:");
        for category in symbols_by_category.keys() {
            println!("  {}", category);
        }
    }
    
    Ok(())
}

fn print_crates_tree(crates: &[(String, Vec<Symbol>)], prefix: &str, depth: u32) {
    for (i, (name, symbols)) in crates.iter().enumerate() {
        let is_last = i == crates.len() - 1;
        let item_prefix = if is_last { "└──" } else { "├──" };
        let child_prefix = if is_last { "    " } else { "│   " };
        
        println!("{}{} {} ({} symbols)", prefix, item_prefix, name, symbols.len());
        
        if depth >= 3 {
            print_symbols_tree(symbols, &format!("{}{}", prefix, child_prefix));
        }
    }
}

fn print_trait_impls_tree(
    trait_impls: &BTreeMap<String, Vec<(String, usize)>>,
    prefix: &str,
    depth: u32,
) {
    let targets: Vec<_> = trait_impls.keys().collect();
    for (i, target_crate) in targets.iter().enumerate() {
        let is_last = i == targets.len() - 1;
        let item_prefix = if is_last { "└──" } else { "├──" };
        let child_prefix = if is_last { "    " } else { "│   " };
        
        let impls = &trait_impls[*target_crate];
        let total_symbols: usize = impls.iter().map(|(_, count)| count).sum();
        
        println!("{}{} {} ({} impls, {} symbols)", 
                prefix, item_prefix, target_crate, impls.len(), total_symbols);
        
        if depth >= 3 {
            for (j, (trait_name, symbol_count)) in impls.iter().enumerate() {
                let is_last_impl = j == impls.len() - 1;
                let impl_prefix = if is_last_impl { "└──" } else { "├──" };
                println!("{}{}{}  {} ({} symbols)", prefix, child_prefix, impl_prefix, trait_name, symbol_count);
            }
        }
    }
}

fn print_compiler_tree(compiler: &[(String, Vec<Symbol>)], prefix: &str, depth: u32) {
    for (i, (name, symbols)) in compiler.iter().enumerate() {
        let is_last = i == compiler.len() - 1;
        let item_prefix = if is_last { "└──" } else { "├──" };
        let child_prefix = if is_last { "    " } else { "│   " };
        
        println!("{}{} {} ({} symbols)", prefix, item_prefix, name, symbols.len());
        
        if depth >= 3 {
            print_symbols_tree(symbols, &format!("{}{}", prefix, child_prefix));
        }
    }
}

fn print_system_tree(
    system_by_type: &BTreeMap<SystemSymbolType, Vec<Symbol>>,
    prefix: &str,
    depth: u32,
) {
    // Since we put all system symbols under one key, get all symbols and group them by dot-prefix
    if let Some(all_system_symbols) = system_by_type.get(&SystemSymbolType::Other("system".to_string())) {
        let grouped_symbols = group_symbols_by_dot_prefix(all_system_symbols);
        print_grouped_symbols_tree(&grouped_symbols, prefix, depth);
    }
}

fn group_symbols_by_dot_prefix(symbols: &[Symbol]) -> BTreeMap<String, Vec<Symbol>> {
    let mut grouped: BTreeMap<String, Vec<Symbol>> = BTreeMap::new();
    
    for symbol in symbols {
        let name = if symbol.is_demangled() {
            &symbol.demangled
        } else {
            &symbol.mangled
        };
        
        // Special handling for GCC_except_table symbols
        let prefix = if name.starts_with("GCC_except_table") {
            "GCC_except_table"
        } else if let Some(dot_pos) = name.find('.') {
            // Extract prefix before first dot, or use the entire name if no dot
            &name[..dot_pos]
        } else {
            name
        };
        
        grouped.entry(prefix.to_string()).or_default().push(symbol.clone());
    }
    
    grouped
}

fn print_grouped_symbols_tree(
    grouped_symbols: &BTreeMap<String, Vec<Symbol>>,
    prefix: &str,
    depth: u32,
) {
    let prefixes: Vec<_> = grouped_symbols.keys().collect();
    
    for (i, group_prefix) in prefixes.iter().enumerate() {
        let is_last = i == prefixes.len() - 1;
        let item_prefix = if is_last { "└──" } else { "├──" };
        let child_prefix = if is_last { "    " } else { "│   " };
        
        let symbols = &grouped_symbols[*group_prefix];
        
        if symbols.len() > 1 {
            // Show group with count
            println!("{}{} {}. ({} symbols)", prefix, item_prefix, group_prefix, symbols.len());
            
            if depth >= 4 {
                // Show individual symbols within the group
                print_symbols_tree(symbols, &format!("{}{}", prefix, child_prefix));
            }
        } else {
            // Show single symbol directly
            let symbol = &symbols[0];
            if symbol.is_demangled() {
                println!("{}{} {} ({})", prefix, item_prefix, symbol.demangled, symbol.mangled);
            } else {
                println!("{}{} {}", prefix, item_prefix, symbol.mangled);
            }
        }
    }
}

fn print_symbols_tree(symbols: &[Symbol], prefix: &str) {
    for (i, symbol) in symbols.iter().enumerate() {
        let is_last = i == symbols.len() - 1;
        let item_prefix = if is_last { "└──" } else { "├──" };
        
        if symbol.is_demangled() {
            println!("{}{} {} ({})", prefix, item_prefix, symbol.demangled, symbol.mangled);
        } else {
            println!("{}{} {}", prefix, item_prefix, symbol.mangled);
        }
    }
}
