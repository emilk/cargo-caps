use anyhow::{Context, Result};
use object::{Object, ObjectSymbol};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;

use crate::{Symbol, SymbolCategory};

/// Check if a file is an rlib archive
pub fn is_rlib(path: &Path) -> Result<bool> {
    let data = fs::read(path).context("Failed to read file")?;
    
    // Check if it's an ar archive (rlib files are ar archives)
    Ok(data.starts_with(b"!<arch>\n"))
}

/// Extract and analyze symbols from an rlib file
pub fn extract_rlib_symbols(
    binary_path: &Path,
    verbose: bool,
    filter_module: Option<&str>,
) -> Result<()> {
    let data = fs::read(binary_path)
        .with_context(|| format!("Failed to read rlib file: {}", binary_path.display()))?;

    let mut archive = ar::Archive::new(Cursor::new(data));
    let mut all_symbols: BTreeMap<SymbolCategory, Vec<Symbol>> = BTreeMap::new();

    // Extract and process each object file in the archive
    while let Some(entry_result) = archive.next_entry() {
        let mut entry = entry_result.context("Failed to read archive entry")?;
        
        // Skip non-object files (like metadata files)
        let header = entry.header();
        let filename = String::from_utf8_lossy(header.identifier());
        
        if !filename.ends_with(".o") {
            continue;
        }

        // Read the object file data
        let mut obj_data = Vec::new();
        entry.read_to_end(&mut obj_data)
            .context("Failed to read object file from archive")?;

        // Parse the object file and extract symbols
        if let Ok(file) = object::File::parse(&*obj_data) {
            for symbol in file.symbols() {
                if let Ok(name) = symbol.name() {
                    if !name.is_empty() {
                        let sym = Symbol::from_mangled(name.to_string());
                        all_symbols
                            .entry(sym.category.clone())
                            .or_default()
                            .push(sym);
                    }
                }
            }
        }
    }

    // Use the same output logic as the main extract_symbols function
    output_symbols(binary_path, all_symbols, verbose, filter_module)
}

fn output_symbols(
    binary_path: &Path,
    symbols_by_category: BTreeMap<SymbolCategory, Vec<Symbol>>,
    verbose: bool,
    filter_module: Option<&str>,
) -> Result<()> {
    // Filter to specific category if requested
    if let Some(filter_name) = filter_module {
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
            println!("Category '{}' not found in rlib", filter_name);
            println!();
            println!("Available categories:");
            // Show available categories for reference
            let category_names: Vec<_> =
                symbols_by_category.keys().map(|c| c.to_string()).collect();
            for name in category_names {
                println!("  {}", name);
            }
        }
    } else if verbose {
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
                    crate::SystemSymbolType::OutlinedFunctions => outlined_total += total_symbols,
                    crate::SystemSymbolType::StubHelpers => stub_helpers_total += total_symbols,
                    crate::SystemSymbolType::LibraryFunctions => library_functions_total += total_symbols,
                    crate::SystemSymbolType::Symbols => symbols_total += total_symbols,
                    crate::SystemSymbolType::Other(_) => other_total += total_symbols,
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