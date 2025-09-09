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
    #[arg(short, long)]
    depth: Option<u32>,

    /// Show symbols for a specific module (by display name)
    #[arg(short, long)]
    module: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if !args.binary_path.exists() {
        anyhow::bail!("Binary file does not exist: {}", args.binary_path.display());
    }

    let symbols = capabara::extract_symbols(&args.binary_path)?;
    let options = PrintOptions {
        depth: args.depth,
        filter_module: args.module.as_deref(),
    };
    capabara::print::print_symbols(&args.binary_path, symbols, options)?;
    Ok(())
}
