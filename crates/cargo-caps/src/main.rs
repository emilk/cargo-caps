use cargo_caps::Commands;
use clap::Parser as _;

#[derive(clap::Parser)]
#[command(name = "cargo-caps")]
#[command(about = "A tool for analyzing capabilities")]
struct Args {
    #[clap(subcommand)]
    command: Commands,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.command {
        Commands::Build(check_cmd) => check_cmd.execute(),
        Commands::Caps(caps_cmd) => caps_cmd.execute(),
        Commands::Init(init_cmd) => init_cmd.execute(),
        Commands::Symbols(symbols_cmd) => symbols_cmd.execute(),
    }
}
