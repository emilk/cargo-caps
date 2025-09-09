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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Module {
    Crate(String),
    TraitImpl {
        trait_for: String,
        target_crate: String,
    },
    Compiler(String),
    System,
    Unknown,
}

impl fmt::Display for Module {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Module::Crate(name) => write!(f, "{}", name),
            Module::TraitImpl {
                trait_for,
                target_crate,
            } => {
                write!(f, "trait_impl: {} → {}", trait_for, target_crate)
            }
            Module::Compiler(name) => write!(f, "compiler: {}", name),
            Module::System => write!(f, "system"),
            Module::Unknown => write!(f, "unknown"),
        }
    }
}

fn classify_symbol(demangled_symbol: &str, original_symbol: &str) -> Module {
    // Handle Rust symbols that were successfully demangled
    if original_symbol != demangled_symbol {
        if let Some(first_colon) = demangled_symbol.find("::") {
            let first_part = &demangled_symbol[..first_colon];

            // Handle trait implementations like _$LT$Type$u20$as$u20$crate..trait$GT$::method
            if first_part.starts_with("_$LT$") && first_part.contains("$u20$as$u20$") {
                if let Some(as_pos) = first_part.find("$u20$as$u20$") {
                    let trait_for = &first_part[5..as_pos]; // Skip "_$LT$"
                    let remaining = &first_part[as_pos + 12..]; // Skip "$u20$as$u20$"

                    // Extract target crate from the remaining part
                    let target_crate = if let Some(dot_dot) = remaining.find("..") {
                        &remaining[..dot_dot]
                    } else if remaining.ends_with("$GT$") {
                        &remaining[..remaining.len() - 4]
                    } else {
                        remaining
                    };

                    return Module::TraitImpl {
                        trait_for: decode_rust_type(trait_for),
                        target_crate: target_crate.to_string(),
                    };
                }
            }

            // Handle compiler-generated symbols
            if first_part.starts_with("__rustc[") {
                return Module::Compiler("rustc".to_string());
            }

            // Regular crate symbol
            return Module::Crate(first_part.to_string());
        }
    }

    // Handle undemangled but potentially Rust symbols
    if original_symbol.starts_with('_') && original_symbol.contains("::") {
        return Module::Unknown; // Could be Rust but failed to demangle
    }

    // System/C symbols
    Module::System
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

    let mut symbols_by_module: BTreeMap<Module, Vec<(String, String)>> = BTreeMap::new();

    for symbol in file.symbols() {
        if let Ok(name) = symbol.name() {
            if !name.is_empty() {
                let demangled = demangle_symbol(name);
                let module = classify_symbol(&demangled, name);

                symbols_by_module
                    .entry(module)
                    .or_default()
                    .push((demangled, name.to_string()));
            }
        }
    }

    // Filter to specific module if requested
    if let Some(filter_name) = filter_module {
        let mut found = false;
        for (module, symbols) in &symbols_by_module {
            if module.to_string().contains(filter_name) {
                println!(
                    "Symbols in {} for module '{}':",
                    binary_path.display(),
                    filter_name
                );
                println!();
                println!(
                    "=== {} ({} symbols) ===",
                    module,
                    symbols.len()
                );

                for (demangled, original) in symbols {
                    if demangled != original {
                        println!("  {} ({})", demangled, original);
                    } else {
                        println!("  {}", original);
                    }
                }
                found = true;
                break;
            }
        }

        if !found {
            println!("Module '{}' not found in binary", filter_name);
            println!();
            println!("Available modules:");
            // Show available modules for reference
            let module_names: Vec<_> = symbols_by_module.keys().map(|m| m.to_string()).collect();
            for name in module_names {
                println!("  {}", name);
            }
        }
    } else if verbose {
        println!("Symbols in {} grouped by module:", binary_path.display());
        println!();

        for (module, symbols) in symbols_by_module {
            println!(
                "=== {} ({} symbols) ===",
                module,
                symbols.len()
            );

            for (demangled, original) in symbols {
                if demangled == original {
                    println!("  {}", original);
                } else {
                    println!("  {} ({})", demangled, original);
                }
            }
            println!();
        }
    } else {
        println!("Modules found in {}:", binary_path.display());
        println!();

        for (module, symbols) in symbols_by_module {
            println!("{} ({} symbols)", module, symbols.len());
        }
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
