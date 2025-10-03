use std::fs;
use std::path::Path;

use anyhow::Context as _;

#[derive(clap::Args)]
pub struct InitCommand;

impl InitCommand {
    #[expect(clippy::unused_self, reason = "we need the self parameter for clap")]
    pub fn execute(self) -> anyhow::Result<()> {
        let config_path = Path::new("cargo-caps.eon");

        if config_path.exists() {
            anyhow::bail!("cargo-caps.eon already exists");
        }

        let default_config = r#"// Configuration file for cargo-caps.
// See https://crates.io/crates/cargo-caps
// The capabilities are additive:
// if a crate matches several rules, the crate is granted
// the union of the capabilities afforded by all the rules.
rules: [
	{
		// Capabilities all crates are allowed:
		crates: ["*"]
		caps: [
            "alloc"   // allocate memory
            "panic"   // call `panic!()`
            "stdio"   // Read/write stdin/stdout/stderr
            "sysinfo" // Read ystem info like env-vars
            "time"    // Tell the time
        ]
	}
	{
		// These crates are allowed to have build.rs files
		caps: ["build.rs"]
		crates: []
	}
]
"#;

        fs::write(config_path, default_config)
            .with_context(|| "Failed to create cargo-caps.eon file")?;

        println!("Created cargo-caps.eon");
        println!("Try running 'cargo-caps check'");
        Ok(())
    }
}
