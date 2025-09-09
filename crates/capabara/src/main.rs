use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use capabara::print::PrintOptions;

#[derive(Parser)]
#[command(name = "capabara")]
#[command(about = "Extract and demangle symbols from macOS binaries")]
struct Args {
    /// Path to the binary file
    binary_path: PathBuf,

    /// Show symbols at given depth (0=summary only, 1=categories, 2=symbols)
    #[arg(short, long, default_value = "2")]
    depth: u32,

    /// Filter to show only symbols under the given tree path (e.g., "crates/std/sync")
    #[arg(short, long)]
    filter: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if !args.binary_path.exists() {
        anyhow::bail!("Binary file does not exist: {}", args.binary_path.display());
    }

    let symbols = capabara::extract_symbols(&args.binary_path)?;
    let options = PrintOptions {
        depth: args.depth,
        filter: args.filter,
    };
    capabara::print::print_symbols(&args.binary_path, symbols, options)?;
    Ok(())
}
