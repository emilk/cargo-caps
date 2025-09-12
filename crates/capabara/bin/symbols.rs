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
    /// Paths to the binary files
    binary_paths: Vec<PathBuf>,

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

    if args.binary_paths.is_empty() {
        // Print help and return error
        use clap::CommandFactory as _;
        Args::command().print_help()?;
        println!();
        anyhow::bail!("At least one binary file path must be provided");
    }

    let options = PrintOptions {
        depth: args.depth,
        filter: args.filter.clone(),
        include_mangled: args.mangled,
        show_metadata: args.show_metadata,
    };

    for (i, binary_path) in args.binary_paths.iter().enumerate() {
        if !binary_path.exists() {
            anyhow::bail!("Binary file does not exist: {}", binary_path.display());
        }

        // Add separator between multiple files
        if i > 0 {
            println!("\n{}", "=".repeat(80));
        }
        
        let symbols = capabara::extract_symbols(binary_path)?;
        let original_count = symbols.len();
        let filtered_symbols = capabara::filter_symbols(symbols, args.include_local, args.include_all_kinds);
        
        if args.show_metadata && filtered_symbols.len() < original_count {
            eprintln!(
                "Filtered {} -> {} symbols (use --include-local and/or --include-all-kinds to show more)",
                original_count, filtered_symbols.len()
            );
        }

        capabara::print::print_symbols(binary_path, &filtered_symbols, &options)?;
    }
    Ok(())
}
