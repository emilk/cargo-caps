pub mod caps;
pub mod check;
pub mod init;
pub mod symbols;

pub use caps::CapsCommand;
pub use check::CheckCommand;
pub use init::InitCommand;
pub use symbols::SymbolCommand;

#[derive(clap::Subcommand)]
pub enum Commands {
    /// Analyze crate capabilities by running cargo build
    #[command(name = "check")]
    Build(CheckCommand),

    /// Extract and analyze capabilities of a particular crate
    #[command(name = "caps")]
    Caps(CapsCommand),

    /// Create a default cargo-caps.eon configuration file
    #[command(name = "init")]
    Init(InitCommand),

    /// Extract and analyze symbols of a binary
    #[command(name = "symbols")]
    Symbols(SymbolCommand),
}
