use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "Celerrate")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "A fast, modern PHP toolchain written in Rust.", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Init,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => println!("Initializing celerrate.toml configuration file..."),
    }
}
