use std::path::PathBuf;

use clap::Parser;

use crate::analyzer::CapsAnalyzer;

#[derive(Parser)]
/// Analyze capabilities of a specific rlib file
pub struct InfoCommand {
    /// Path to the rlib file to analyze
    pub rlib_path: PathBuf,

    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,
}

impl InfoCommand {
    pub fn execute(&self) -> anyhow::Result<()> {
        if !self.rlib_path.exists() {
            anyhow::bail!("Rlib file does not exist: {}", self.rlib_path.display());
        }

        let crate_name = self.rlib_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        let mut analyzer = CapsAnalyzer::new();

        let utf8_path = cargo_metadata::camino::Utf8PathBuf::from_path_buf(self.rlib_path.clone())
            .map_err(|p| anyhow::anyhow!("Invalid UTF-8 path: {}", p.display()))?;
        analyzer.add_lib_or_bin(crate_name, &utf8_path, self.verbose, None);

        Ok(())
    }
}