use cargo_caps::Commands;
use clap::Parser;

#[derive(Parser)]
#[command(name = "cargo-caps")]
#[command(about = "A tool for analyzing capabilities")]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.command {
        Commands::Symbols(symbols_cmd) => symbols_cmd.execute(),
        Commands::Build(build_cmd) => build_cmd.execute(),
        Commands::Caps(caps_cmd) => caps_cmd.execute(),
    }
}
