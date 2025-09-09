use std::{collections::BTreeMap, path::Path};

use anyhow::Result;

use crate::symbol::{Symbol, SymbolCategory, SystemSymbolType};

pub struct PrintOptions<'a> {
    pub verbose: bool,
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
        let mut found = false;
        for (category, symbols) in &symbols_by_category {
            if category.to_string().contains(filter_name) {
                println!(
                    "Symbols in {} for category '{}':",
                    binary_path.display(),
                    filter_name
                );
                println!();
                println!("=== {} ({} symbols) ===", category, symbols.len());

                for symbol in symbols {
                    if symbol.is_demangled() {
                        println!("  {} ({})", symbol.demangled, symbol.mangled);
                    } else {
                        println!("  {}", symbol.mangled);
                    }
                }
                found = true;
                break;
            }
        }

        if !found {
            println!("Category '{}' not found", filter_name);
            println!();
            println!("Available categories:");
            // Show available categories for reference
            let category_names: Vec<_> =
                symbols_by_category.keys().map(|c| c.to_string()).collect();
            for name in category_names {
                println!("  {}", name);
            }
        }
    } else if options.verbose {
        println!("Symbols in {} grouped by category:", binary_path.display());
        println!();

        for (category, symbols) in symbols_by_category {
            println!("=== {} ({} symbols) ===", category, symbols.len());

            for symbol in symbols {
                if symbol.is_demangled() {
                    println!("  {} ({})", symbol.demangled, symbol.mangled);
                } else {
                    println!("  {}", symbol.mangled);
                }
            }
            println!();
        }
    } else {
        println!("Symbol categories found in {}:", binary_path.display());
        println!();

        // Separate different types of categories
        let mut crates = Vec::new();
        let mut trait_impls_by_target = BTreeMap::new();
        let mut compiler = Vec::new();
        let mut system_by_type = BTreeMap::new();
        let mut unknown = Vec::new();

        for (category, symbols) in &symbols_by_category {
            match category {
                SymbolCategory::Crate(name) => crates.push((name.clone(), symbols.len())),
                SymbolCategory::TraitImpl { target_crate, .. } => {
                    trait_impls_by_target
                        .entry(target_crate.clone())
                        .or_insert_with(Vec::new)
                        .push(symbols.len());
                }
                SymbolCategory::Compiler(name) => compiler.push((name.clone(), symbols.len())),
                SymbolCategory::System(sys_type) => {
                    system_by_type
                        .entry(sys_type.clone())
                        .or_insert_with(Vec::new)
                        .push(symbols.len());
                }
                SymbolCategory::Unknown => unknown.push(("unknown".to_string(), symbols.len())),
            }
        }

        // Print crates
        println!("# Crates ({}):", crates.len());
        crates.sort();
        for (name, count) in &crates {
            println!("  {} ({} symbols)", name, count);
        }

        // Print trait implementations grouped by target crate
        if !trait_impls_by_target.is_empty() {
            println!();
            println!("# Trait implementations by target crate:");
            for (target_crate, symbol_counts) in &trait_impls_by_target {
                let total_symbols: usize = symbol_counts.iter().sum();
                let impl_count = symbol_counts.len();
                println!(
                    "  trait_impl → {} ({} impls, {} symbols total)",
                    target_crate, impl_count, total_symbols
                );
            }
        }

        // Print other categories
        if !compiler.is_empty() {
            println!();
            println!("# Compiler:");
            for (name, count) in &compiler {
                println!("  {} ({} symbols)", name, count);
            }
        }

        if !system_by_type.is_empty() {
            println!();
            println!("# System:");

            let mut outlined_total = 0;
            let mut stub_helpers_total = 0;
            let mut library_functions_total = 0;
            let mut symbols_total = 0;
            let mut other_total = 0;

            for (sys_type, symbol_counts) in &system_by_type {
                let total_symbols: usize = symbol_counts.iter().sum();
                match sys_type {
                    SystemSymbolType::OutlinedFunctions => outlined_total += total_symbols,
                    SystemSymbolType::StubHelpers => stub_helpers_total += total_symbols,
                    SystemSymbolType::LibraryFunctions => library_functions_total += total_symbols,
                    SystemSymbolType::Symbols => symbols_total += total_symbols,
                    SystemSymbolType::Other(_) => other_total += total_symbols,
                }
            }

            if outlined_total > 0 {
                println!("  outlined functions ({} symbols)", outlined_total);
            }
            if stub_helpers_total > 0 {
                println!("  stub helpers ({} symbols)", stub_helpers_total);
            }
            if library_functions_total > 0 {
                println!("  library functions ({} symbols)", library_functions_total);
            }
            if symbols_total > 0 {
                println!("  symbols ({} symbols)", symbols_total);
            }
            if other_total > 0 {
                println!("  other ({} symbols)", other_total);
            }
        }

        if !unknown.is_empty() {
            println!();
            println!("# Unknown:");
            for (name, count) in &unknown {
                println!("  {} ({} symbols)", name, count);
            }
        }

        // Print summary
        let total_symbols: usize = symbols_by_category.values().map(|v| v.len()).sum();

        println!();
        println!("# Summary:");
        println!("  {} crates", crates.len());
        println!(
            "  {} trait implementations",
            trait_impls_by_target
                .values()
                .map(|v| v.len())
                .sum::<usize>()
        );
        println!("  {} total symbols", total_symbols);
    }

    Ok(())
}
