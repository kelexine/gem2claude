// CLI module for gem2claude
// Author: kelexine (https://github.com/kelexine)

use clap::Parser;

/// gem2claude - OAuth-based Gemini API to Claude Code Compatible Proxy
#[derive(Parser, Debug)]
#[command(name = "gem2claude", version, about, long_about = None)]
pub struct Args {
    /// Run OAuth login flow (opens browser, then starts server)
    #[arg(long)]
    pub login: bool,
}
