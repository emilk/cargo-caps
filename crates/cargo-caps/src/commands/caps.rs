use cargo_metadata::camino::Utf8PathBuf;

use crate::{
    cap_rule::SymbolRules,
    capability::{Capability, DeducedCapabilities},
    reservoir_sample::ReservoirSampleExt as _,
};

#[derive(clap::Parser)]
pub struct CapsCommand {
    /// Path to the binary file
    pub binary_path: Utf8PathBuf,

    /// Include local compilation symbols (excluded by default)
    #[arg(long, default_value = "false")]
    pub include_local: bool,

    /// Include all symbol kinds (by default, only executable code symbols are shown)
    #[arg(long, default_value = "false")]
    pub include_all_kinds: bool,

    /// Show detailed reasoning for each capability
    #[arg(short, long, default_value = "false")]
    pub verbose: bool,
}

impl CapsCommand {
    pub fn execute(&self) -> anyhow::Result<()> {
        if !self.binary_path.exists() {
            anyhow::bail!("Binary file does not exist: {}", self.binary_path);
        }

        let rules = SymbolRules::load_default();

        // Extract symbols from the binary
        let symbols = crate::extract_symbols(&self.binary_path)?;
        let filtered_symbols =
            crate::filter_symbols(symbols, self.include_local, self.include_all_kinds);

        // Analyze capabilities
        let capabilities = DeducedCapabilities::from_symbols(&rules, filtered_symbols)?;

        // Print results
        self.print_capabilities(&capabilities);

        Ok(())
    }

    fn print_capabilities(&self, capabilities: &DeducedCapabilities) {
        println!("Capability Analysis for: {}", self.binary_path);
        println!("═══════════════════════════════════════");

        if capabilities.own_caps.is_empty() {
            println!("🔒 No specific capabilities detected");
        } else {
            println!("🔍 Detected Capabilities:");
            println!();

            for (capability, reasons) in &capabilities.own_caps {
                let icon = capability.emoji();
                println!("  {icon} {capability:?}");

                if (self.verbose || *capability == Capability::Any) && !reasons.is_empty() {
                    println!("    Reasons ({}):", reasons.len());
                    for reason in reasons.iter().reservoir_sample(5) {
                        println!("      • {reason}");
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
            for (crate_name, reasons) in &capabilities.unknown_crates {
                println!("  📦 {} ({} symbols)", crate_name, reasons.len());
                if self.verbose {
                    for reason in reasons.iter().take(3) {
                        println!("      • {reason}");
                    }
                    if reasons.len() > 3 {
                        println!("      ... and {} more", reasons.len() - 3);
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
            if self.verbose {
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
}
