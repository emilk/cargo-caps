use clap::Parser;

pub mod analyzer;
pub mod build;
pub mod info;

pub use analyzer::CapsAnalyzer;
pub use build::BuildCommand;
pub use info::InfoCommand;

#[derive(Parser)]
pub enum Commands {
    /// Analyze capabilities by running cargo build
    Build(BuildCommand),
    /// Analyze capabilities of a specific rlib file
    Info(InfoCommand),
}