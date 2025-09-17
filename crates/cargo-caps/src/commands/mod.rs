pub mod build;
pub mod caps;
pub mod symbols;

pub use build::BuildCommand;
pub use caps::CapsCommand;
pub use symbols::SymbolCommand;

#[derive(clap::Subcommand)]
pub enum Commands {
    /// Analyze crate capabilities by running cargo build
    #[command(name = "build")]
    Build(BuildCommand),

    /// Extract and analyze capabilities of a particular crate
    #[command(name = "caps")]
    Caps(CapsCommand),

    /// Extract and analyze symbols of a binary
    #[command(name = "symbols")]
    Symbols(SymbolCommand),
}
