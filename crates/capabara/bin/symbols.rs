use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use capabara::print::PrintOptions;

#[derive(Parser)]
#[command(name = "capabara")]
#[command(about = "Extract and demangle symbols from macOS binaries")]
#[command(long_about = "Extract and demangle symbols from macOS binaries.
By default, only executable code symbols (functions/labels) and unknown symbols from static/dynamic scope are shown.
Use --include-local to show compilation-local symbols.
Use --include-all-kinds to show data, section, and other non-executable symbols.")]
struct Args {
    /// Path to the binary file
    binary_path: PathBuf,

    /// Show symbols at given depth (0=summary only, 1=categories, 2=symbols)
    #[arg(short, long, default_value = "2")]
    depth: u32,

    /// Filter to show only symbols under the given tree path (e.g., "crates/std/sync")
    #[arg(short, long)]
    filter: Option<String>,

    /// Include mangled symbol names alongside demangled names
    #[arg(short = 'm', long, default_value = "false")]
    mangled: bool,

    /// Show symbol metadata (scope and kind)
    #[arg(long, default_value = "false")]
    show_metadata: bool,

    /// Include local compilation symbols (excluded by default)
    #[arg(long, default_value = "false")]
    include_local: bool,

    /// Include all symbol kinds (by default, only executable code symbols are shown)
    #[arg(long, default_value = "false")]
    include_all_kinds: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if !args.binary_path.exists() {
        anyhow::bail!("Binary file does not exist: {}", args.binary_path.display());
    }

    let symbols = capabara::extract_symbols(&args.binary_path)?;
    let original_count = symbols.len();
    let filtered_symbols = capabara::filter_symbols(symbols, args.include_local, args.include_all_kinds);
    
    if args.show_metadata && filtered_symbols.len() < original_count {
        eprintln!(
            "Filtered {} -> {} symbols (use --include-local and/or --include-all-kinds to show more)",
            original_count, filtered_symbols.len()
        );
    }

    let options = PrintOptions {
        depth: args.depth,
        filter: args.filter,
        include_mangled: args.mangled,
        show_metadata: args.show_metadata,
    };
    capabara::print::print_symbols(&args.binary_path, &filtered_symbols, &options)?;
    Ok(())
}
