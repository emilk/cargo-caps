use clap::Parser;

pub mod build;
pub mod caps;
pub mod symbols;

pub use build::BuildCommand;
pub use caps::CapsCommand;
pub use symbols::SymbolCommand;

#[derive(Parser)]
pub enum Commands {
    /// Analyze capabilities by running cargo build
    Build(BuildCommand),
    /// Extract and analyze symbols from binaries
    Symbols(SymbolCommand),
    /// Extract and analyze capabilities from a single binary
    Caps(CapsCommand),
}