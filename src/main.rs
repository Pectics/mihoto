mod application;
mod cli;
mod domain;
mod infrastructure;

use colored::Colorize;

#[tokio::main]
async fn main() {
    if let Err(error) = cli::dispatch::run().await {
        eprintln!("{} {}", "error:".bright_red().bold(), error);
        std::process::exit(1);
    }
}
