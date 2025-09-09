use anyhow::{Context, Result};
use clap::Parser;
use object::{Object, ObjectSymbol};
use std::collections::BTreeMap;
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

fn extract_crate_name(demangled_symbol: &str) -> &str {
    // For Rust symbols, the crate name is typically the first component
    if let Some(first_colon) = demangled_symbol.find("::") {
        let crate_part = &demangled_symbol[..first_colon];
        
        // Handle generic syntax like _$LT$crate..module..Type$u20$as$u20$...
        if crate_part.starts_with("_$LT$") {
            if let Some(first_dot_dot) = crate_part.find("..") {
                return &crate_part[5..first_dot_dot];
            }
        }
        
        return crate_part;
    }
    
    "unknown"
}

fn extract_symbols(binary_path: &PathBuf, verbose: bool) -> Result<()> {
    let data = fs::read(binary_path)
        .with_context(|| format!("Failed to read binary file: {}", binary_path.display()))?;
    
    let file = object::File::parse(&*data)
        .with_context(|| "Failed to parse binary file")?;
    
    let mut symbols_by_crate: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    
    for symbol in file.symbols() {
        if let Ok(name) = symbol.name() {
            if !name.is_empty() {
                let demangled = demangle_symbol(name);
                let crate_name = if name != demangled {
                    // This is a demangled Rust symbol
                    extract_crate_name(&demangled).to_string()
                } else if name.starts_with('_') && name.contains("::") {
                    // This might be a mangled symbol we couldn't demangle but looks like Rust
                    "rust_undemangled".to_string()
                } else {
                    // System/C symbols
                    "system".to_string()
                };
                
                symbols_by_crate
                    .entry(crate_name)
                    .or_insert_with(Vec::new)
                    .push((demangled, name.to_string()));
            }
        }
    }
    
    if verbose {
        println!("Symbols in {} grouped by crate:", binary_path.display());
        println!();
        
        for (crate_name, symbols) in symbols_by_crate {
            println!("=== {} ({} symbols) ===", crate_name, symbols.len());
            
            for (demangled, original) in symbols {
                if demangled != original {
                    println!("  {} ({})", demangled, original);
                } else {
                    println!("  {}", original);
                }
            }
            println!();
        }
    } else {
        println!("Crates found in {}:", binary_path.display());
        println!();
        
        for (crate_name, symbols) in symbols_by_crate {
            println!("{} ({} symbols)", crate_name, symbols.len());
        }
    }
    
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    
    if !args.binary_path.exists() {
        anyhow::bail!("Binary file does not exist: {}", args.binary_path.display());
    }
    
    extract_symbols(&args.binary_path, args.verbose)?;
    Ok(())
}