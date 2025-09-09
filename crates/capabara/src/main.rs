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

    /// Show all symbols within each crate (default: only show crate names)
    #[arg(short, long)]
    verbose: bool,

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
    let options = {
        let verbose = args.verbose;
        let filter_module = args.module.as_deref();
        PrintOptions {
            verbose,
            filter_module,
        }
    };
    capabara::print::print_symbols(&args.binary_path, symbols, options)?;
    Ok(())
}
