use anyhow::Result;

pub mod cli;
pub mod errors;
pub mod model;
pub mod pack;
pub mod parse;
pub mod render;
pub mod search;
pub mod tokens;

pub fn run() -> Result<()> {
    cli::run()
}
