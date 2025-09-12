use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use capabara::capability::{Capability, DeducedCapablities};

#[derive(Parser)]
#[command(name = "capability")]
#[command(about = "Extract symbols from binaries and analyze their capabilities")]
#[command(
    long_about = "Extract symbols from macOS binaries and analyze what capabilities they suggest.
This tool examines the symbols in a binary and deduces what capabilities the code might have,
such as memory allocation, network access, file operations, etc."
)]
struct Args {
    /// Path to the binary file
    binary_path: PathBuf,

    /// Include local compilation symbols (excluded by default)
    #[arg(long, default_value = "false")]
    include_local: bool,

    /// Include all symbol kinds (by default, only executable code symbols are shown)
    #[arg(long, default_value = "false")]
    include_all_kinds: bool,

    /// Show detailed reasoning for each capability
    #[arg(short, long, default_value = "false")]
    verbose: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if !args.binary_path.exists() {
        anyhow::bail!("Binary file does not exist: {}", args.binary_path.display());
    }

    // Extract symbols from the binary
    let symbols = capabara::extract_symbols(&args.binary_path)?;
    let filtered_symbols =
        capabara::filter_symbols(symbols, args.include_local, args.include_all_kinds);

    // Analyze capabilities
    let capabilities = DeducedCapablities::from_symbols(filtered_symbols);

    // Print results
    print_capabilities(&args.binary_path, &capabilities, args.verbose);

    Ok(())
}

fn print_capabilities(
    binary_path: &std::path::Path,
    capabilities: &DeducedCapablities,
    verbose: bool,
) {
    println!("Capability Analysis for: {}", binary_path.display());
    println!("═══════════════════════════════════════");

    if capabilities.own_caps.is_empty() {
        println!("🔒 No specific capabilities detected");
    } else {
        println!("🔍 Detected Capabilities:");
        println!();

        for (capability, reasons) in &capabilities.own_caps {
            let icon = capability.emoji();
            println!("  {icon} {capability:?}");

            if (verbose || *capability == Capability::Any) && !reasons.is_empty() {
                println!("    Reasons ({} symbols):", reasons.len());
                for symbol in reasons.iter().take(5) {
                    println!("      • {}", symbol.format(false));
                }
                if reasons.len() > 5 {
                    println!("      ... and {} more", reasons.len() - 5);
                }
                println!();
            }
        }
    }

    // Show unknown crates
    if !capabilities.unknown_crates.is_empty() {
        println!("❓ Unknown External Crates:");
        for (crate_name, symbols) in &capabilities.unknown_crates {
            println!("  📦 {} ({} symbols)", crate_name, symbols.len());
            if verbose {
                for symbol in symbols.iter().take(3) {
                    println!("      • {}", symbol.format(false));
                }
                if symbols.len() > 3 {
                    println!("      ... and {} more", symbols.len() - 3);
                }
            }
        }
        println!();
    }

    // Show unknown symbols
    if !capabilities.unknown_symbols.is_empty() {
        println!(
            "🤷 Unclassified Symbols: {}",
            capabilities.unknown_symbols.len()
        );
        if verbose {
            for symbol in capabilities.unknown_symbols.iter().take(10) {
                println!("  • {}", symbol.format(false));
            }
            if capabilities.unknown_symbols.len() > 10 {
                println!("  ... and {} more", capabilities.unknown_symbols.len() - 10);
            }
        }
        println!();
    }

    // Summary
    let total_capabilities = capabilities.own_caps.len();
    let total_unknown_crates = capabilities.unknown_crates.len();
    let total_unknown_symbols = capabilities.unknown_symbols.len();

    println!("📊 Summary:");
    println!("  • Capabilities detected: {total_capabilities}");
    println!("  • External crates: {total_unknown_crates}");
    println!("  • Unclassified symbols: {total_unknown_symbols}");
}
