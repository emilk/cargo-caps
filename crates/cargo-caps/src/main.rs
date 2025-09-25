use cargo_caps::Commands;
use clap::Parser as _;

#[derive(clap::Parser)]
#[command(name = "cargo-caps")]
#[command(about = "A tool for analyzing capabilities")]
struct Args {
    #[clap(subcommand)]
    command: Commands,
}

fn main() {
    #![allow(clippy::exit, reason = "we want to exit with code 1 on error")] // --> we could also return a Result from main so its more clear?

    env_logger::init();

    let args = Args::parse();

    let result = match args.command {
        Commands::Build(check_cmd) => check_cmd.execute(),
        Commands::Caps(caps_cmd) => caps_cmd.execute(),
        Commands::Init(init_cmd) => init_cmd.execute(),
        Commands::Symbols(symbols_cmd) => symbols_cmd.execute(),
    };

    match result {
        Ok(()) => {}
        Err(err) => {
            eprintln!("{err:#}");

            std::process::exit(1);
        }
    }
}
