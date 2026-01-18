//! Command-line argument parsing.
//!
//! This module defines the structure for parsing command-line arguments
//! using `clap`. It acts as the single source of truth for all CLI flags
//! and options supported by the `gem2claude` binary.
//!
//! # Examples
//!
//! ```bash
//! # Run normally
//! gem2claude
//!
//! # Run with initial login
//! gem2claude --login
//! ```

// Author: kelexine (https://github.com/kelexine)

use clap::Parser;

/// gem2claude - OAuth-based Gemini API to Claude Code Compatible Proxy.
///
/// A bridge that allows Anthropic's Claude Code CLI to communicate with
/// Google's internal Gemini API, handling protocol translation,
/// authentication, and context caching.
#[derive(Parser, Debug)]
#[command(name = "gem2claude", version, about, long_about = None)]
pub struct Args {
    /// Run the interactive OAuth login flow.
    ///
    /// 1. Open the system browser to the Google OAuth consent screen.
    /// 2. Listen on a local port for the callback code.
    /// 3. Exchange the code for a refresh token.
    /// 4. Save the credentials to `~/.config/gem2claude/credentials.json`.
    /// 5. Continue starting the server normally.
    #[arg(long)]
    pub login: bool,
}
