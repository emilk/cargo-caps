use clap::Parser;
use cargo_caps::Commands;

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
        Commands::Info(info_cmd) => info_cmd.execute(),
        Commands::Build(build_cmd) => build_cmd.execute(),
    }
}