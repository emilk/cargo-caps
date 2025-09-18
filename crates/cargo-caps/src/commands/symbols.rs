use cargo_metadata::camino::Utf8PathBuf;

use crate::print::PrintOptions;

#[derive(clap::Parser)]
pub struct SymbolCommand {
    /// Paths to the binary files
    pub binary_paths: Vec<Utf8PathBuf>,

    /// Show symbols at given depth (0=summary only, 1=categories, 2=symbols)
    #[arg(short, long, default_value = "2")]
    pub depth: u32,

    /// Filter to show only symbols under the given tree path (e.g., "crates/std/sync")
    #[arg(short, long)]
    pub filter: Option<String>,

    /// Include mangled symbol names alongside demangled names
    #[arg(short = 'm', long, default_value = "false")]
    pub mangled: bool,

    /// Show symbol metadata (scope and kind)
    #[arg(long, default_value = "false")]
    pub show_metadata: bool,

    /// Include local compilation symbols (excluded by default)
    #[arg(long, default_value = "false")]
    pub include_local: bool,

    /// Include all symbol kinds (by default, only executable code symbols are shown)
    #[arg(long, default_value = "false")]
    pub include_all_kinds: bool,
}

impl SymbolCommand {
    pub fn execute(&self) -> anyhow::Result<()> {
        if self.binary_paths.is_empty() {
            // Print help and return error
            use clap::CommandFactory as _;
            Self::command().print_help()?;
            println!();
            anyhow::bail!("At least one binary file path must be provided");
        }

        let options = PrintOptions {
            depth: self.depth,
            filter: self.filter.clone(),
            include_mangled: self.mangled,
            show_metadata: self.show_metadata,
        };

        for (i, binary_path) in self.binary_paths.iter().enumerate() {
            if !binary_path.exists() {
                anyhow::bail!("Binary file does not exist: {binary_path}");
            }

            // Add separator between multiple files
            if i > 0 {
                println!("\n{}", "=".repeat(80));
            }

            let symbols = crate::extract_symbols(binary_path)?;
            let original_count = symbols.len();
            let filtered_symbols =
                crate::filter_symbols(symbols, self.include_local, self.include_all_kinds);

            if self.show_metadata && filtered_symbols.len() < original_count {
                println!(
                    "Filtered {} -> {} symbols (use --include-local and/or --include-all-kinds to show more)",
                    original_count,
                    filtered_symbols.len()
                );
            }

            crate::print::print_symbols(binary_path, &filtered_symbols, &options);
        }
        Ok(())
    }
}
