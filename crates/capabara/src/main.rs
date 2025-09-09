use anyhow::{Context, Result};
use clap::Parser;
use object::{Object, ObjectSymbol};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "capabara")]
#[command(about = "Extract and demangle symbols from macOS binaries")]
struct Args {
    /// Path to the binary file
    binary_path: PathBuf,

    /// Show all symbols within each crate (default: only show crate names)
    #[arg(short, long)]
    verbose: bool,

    /// Show symbols for a specific module (by display name)
    #[arg(short, long)]
    module: Option<String>,
}

fn demangle_symbol(name: &str) -> String {
    if let Ok(demangled) = cpp_demangle::Symbol::new(name) {
        return demangled.to_string();
    }

    if let Ok(demangled) = rustc_demangle::try_demangle(name) {
        return demangled.to_string();
    }

    name.to_string()
}

#[derive(Debug, Clone)]
struct Symbol {
    mangled: String,
    demangled: String,
    category: SymbolCategory,
}

impl Symbol {
    fn from_mangled(mangled: String) -> Self {
        let demangled = demangle_symbol(&mangled);
        let category = classify_symbol(&demangled, &mangled);
        Self {
            mangled,
            demangled,
            category,
        }
    }

    fn is_demangled(&self) -> bool {
        self.mangled != self.demangled
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum SymbolCategory {
    Crate(String),
    TraitImpl {
        trait_for: String,
        target_crate: String,
    },
    Compiler(String),
    System(SystemSymbolType),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum SystemSymbolType {
    OutlinedFunctions,
    StubHelpers,
    LibraryFunctions,
    Symbols,
    Other(String),
}

impl fmt::Display for SymbolCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SymbolCategory::Crate(name) => write!(f, "{}", name),
            SymbolCategory::TraitImpl {
                trait_for,
                target_crate,
            } => {
                write!(f, "trait_impl: {} → {}", trait_for, target_crate)
            }
            SymbolCategory::Compiler(name) => write!(f, "compiler: {}", name),
            SymbolCategory::System(sys_type) => match sys_type {
                SystemSymbolType::OutlinedFunctions => write!(f, "system: outlined functions"),
                SystemSymbolType::StubHelpers => write!(f, "system: stub helpers"),
                SystemSymbolType::LibraryFunctions => write!(f, "system: library functions"),
                SystemSymbolType::Symbols => write!(f, "system: symbols"),
                SystemSymbolType::Other(name) => write!(f, "system: {}", name),
            },
            SymbolCategory::Unknown => write!(f, "unknown"),
        }
    }
}

fn classify_symbol(demangled_symbol: &str, original_symbol: &str) -> SymbolCategory {
    // Handle Rust symbols that were successfully demangled
    if original_symbol != demangled_symbol
        && let Some(first_colon) = demangled_symbol.find("::")
    {
        let first_part = &demangled_symbol[..first_colon];

        // Handle trait implementations like _$LT$Type$u20$as$u20$crate..trait$GT$::method
        if first_part.starts_with("_$LT$")
            && first_part.contains("$u20$as$u20$")
            && let Some(as_pos) = first_part.find("$u20$as$u20$")
        {
            let trait_for = &first_part[5..as_pos]; // Skip "_$LT$"
            let remaining = &first_part[as_pos + 12..]; // Skip "$u20$as$u20$"

            // Extract target crate from the remaining part
            let target_crate = if let Some(dot_dot) = remaining.find("..") {
                &remaining[..dot_dot]
            } else if let Some(stripped) = remaining.strip_suffix("$GT$") {
                stripped
            } else {
                remaining
            };

            return SymbolCategory::TraitImpl {
                trait_for: decode_rust_type(trait_for),
                target_crate: target_crate.to_string(),
            };
        }

        // Handle compiler-generated symbols
        if first_part.starts_with("__rustc[") {
            return SymbolCategory::Compiler("rustc".to_string());
        }

        // Regular crate symbol
        return SymbolCategory::Crate(first_part.to_string());
    }

    // Handle undemangled but potentially Rust symbols
    if original_symbol.starts_with('_') && original_symbol.contains("::") {
        return SymbolCategory::Unknown; // Could be Rust but failed to demangle
    }

    // System/C symbols - classify by pattern
    let sys_type = SystemSymbolType::from_symbol(original_symbol);
    SymbolCategory::System(sys_type)
}

impl SystemSymbolType {
    fn from_symbol(symbol: &str) -> Self {
        if symbol.starts_with("_OUTLINED_FUNCTION_") {
            SystemSymbolType::OutlinedFunctions
        } else if symbol.contains("stub_helper") {
            SystemSymbolType::StubHelpers
        } else if symbol.starts_with('_')
            && (symbol.contains("printf")
                || symbol.contains("malloc")
                || symbol.contains("free")
                || symbol.contains("memcpy")
                || symbol.contains("strlen")
                || symbol.contains("strcmp")
                || symbol.contains("pthread")
                || symbol.starts_with("_lib")
                || symbol.starts_with("_LC_")
                || symbol.contains("objc_")
                || symbol.contains("dyld_"))
        {
            SystemSymbolType::LibraryFunctions
        } else if symbol.starts_with('_')
            && (symbol.contains("GLOBAL_OFFSET_TABLE")
                || symbol.contains("_data")
                || symbol.contains("_bss")
                || symbol.contains("_text")
                || symbol.starts_with("_l")
                || symbol.starts_with("_L"))
        {
            SystemSymbolType::Symbols
        } else {
            SystemSymbolType::Other(symbol.to_string())
        }
    }
}

fn decode_rust_type(encoded: &str) -> String {
    encoded
        .replace("$BP$", "*")
        .replace("$RF$", "&")
        .replace("$LP$", "(")
        .replace("$RP$", ")")
        .replace("$u5b$", "[")
        .replace("$u5d$", "]")
        .replace("$u20$", " ")
        .replace("$LT$", "<")
        .replace("$GT$", ">")
        .replace("$C$", ",")
}

fn extract_symbols(
    binary_path: &PathBuf,
    verbose: bool,
    filter_module: Option<&str>,
) -> Result<()> {
    let data = fs::read(binary_path)
        .with_context(|| format!("Failed to read binary file: {}", binary_path.display()))?;

    let file = object::File::parse(&*data).with_context(|| "Failed to parse binary file")?;

    let mut symbols_by_category: BTreeMap<SymbolCategory, Vec<Symbol>> = BTreeMap::new();

    for symbol in file.symbols() {
        if let Ok(name) = symbol.name()
            && !name.is_empty()
        {
            let sym = Symbol::from_mangled(name.to_string());
            symbols_by_category
                .entry(sym.category.clone())
                .or_default()
                .push(sym);
        }
    }

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
            println!("Category '{}' not found in binary", filter_name);
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

fn main() -> Result<()> {
    let args = Args::parse();

    if !args.binary_path.exists() {
        anyhow::bail!("Binary file does not exist: {}", args.binary_path.display());
    }

    extract_symbols(&args.binary_path, args.verbose, args.module.as_deref())?;
    Ok(())
}
