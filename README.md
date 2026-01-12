# gem2claude

**OAuth-based Gemini API to Claude Code Compatible Proxy**

*Author: kelexine (https://github.com/kelexine)*

## Overview

`gem2claude` is a high-performance Rust proxy that enables [Claude Code](https://github.com/anthropics/anthropic-tools) to work seamlessly with Google's Gemini models via OAuth authentication. If you have a Google Pro subscription with access to Gemini models but want to use Claude Code's superior terminal agent capabilities, this proxy is your solution.

## Key Features

- ✅ **OAuth Integration** - Uses your existing Gemini CLI OAuth credentials
- ✅ **Internal API Support** - Connects to Google's Cloud Code API (`cloudcode-pa.googleapis.com`)
- ✅ **Full API Translation** - Converts between Anthropic and Gemini API formats
- ✅ **Production-Grade** - Enterprise-level error handling, logging, and security
- ✅ **Zero-Copy Streaming** - Efficient SSE streaming with minimal overhead
- ✅ **Automatic Token Refresh** - Never worry about expired OAuth tokens

## Prerequisites

Before using gem2claude, you need:

1. **Google Pro Subscription** with access to Gemini models
2. **Gemini CLI** installed and authenticated via OAuth:
   ```bash
   # Install Gemini CLI if you haven't already
   # Then authenticate
   gemini auth login
   ```
   
   This creates `~/.gemini/oauth_creds.json` which gem2claude uses.

## Installation

### From Source (Current)

```bash
# Clone the repository
git clone https://github.com/kelexine/gem2claude
cd gem2claude

# Build release binary
cargo build --release

# Install to local bin
cp target/release/gem2claude ~/.local/bin/

# Ensure ~/.local/bin is in your PATH
```

### From Binary (Coming Soon)

Pre-built binaries will be available for Linux, macOS, and Windows.

## Quick Start

1. **Verify you have OAuth credentials:**
   ```bash
   ls -la ~/.gemini/oauth_creds.json
   # Should show: -rw------- (600 permissions)
   ```

2. **Run the proxy:**
   ```bash
   gem2claude
   ```
   
   You should see:
   ```
   INFO  Starting gem2claude v0.1.0
   INFO  Loaded OAuth credentials from ~/.gemini/oauth_creds.json
   INFO  Resolving Gemini Cloud Code project ID...
   INFO  Project ID resolved: gen-lang-client-XXXXXXXXXX
   INFO  Starting server on 127.0.0.1:8080
   ```

3. **Configure Claude Code:**
   ```bash
   export ANTHROPIC_BASE_URL="http://localhost:8080"
   export ANTHROPIC_AUTH_TOKEN="dummy" # Not used but required
   ```

4. **Use Claude Code normally:**
   ```bash
   claude "Write a Python script to analyze CSV files"
   ```

## Configuration

Create `~/.gemini-proxy/config.toml` to customize settings:

```toml
[server]
host = "127.0.0.1"
port = 8080

[oauth]
credentials_path = "~/.gemini/oauth_creds.json"
auto_refresh = true
refresh_buffer_seconds = 300  # Refresh 5min before expiry

[gemini]
api_base_url = "https://cloudcode-pa.googleapis.com/v1internal"
default_model = "gemini-3-flash-preview"
timeout_seconds = 300

[logging]
level = "info" # trace, debug, info, warn, error
format = "pretty" # pretty or json
```

## Health Check

Check if the proxy is running correctly:

```bash
curl http://localhost:8080/health | jq '.'
```

Example response:
```json
{
  "status": "healthy",
  "checks": {
    "oauth_credentials": {
      "status": "ok",
      "message": "Valid token, expires in 3500 seconds"
    },
    "project_resolution": {
      "status": "ok",
      "message": "Project ID: gen-lang-client-012345678"
    }
  },
  "timestamp": "2026-01-11T15:00:00Z"
}
```

## Architecture

See [architecture.md](architecture.md) for detailed technical architecture.

**High-level flow:**
```
Claude Code → gem2claude (localhost:8080) → Google Cloud Code API
             [Anthropic→Gemini translation]
```

## Supported Models

gem2claude automatically maps Claude models to Gemini equivalents:

| Claude Model | Gemini Model |
|-------------|--------------|
| claude-opus-4 | gemini-3-pro-preview |
| claude-sonnet-4-5 | gemini-3-flash-preview |
| claude-haiku-4 | gemini-2.5-flash-lite |

## Development

```bash
# Run in development mode with debug logging
RUST_LOG=debug cargo run

# Run tests
cargo test

# Run with hot reload (requires cargo-watch)
cargo watch -x run

# Format code
cargo fmt

# Check for issues
cargo clippy
```

## Troubleshooting

### "Credentials file not found"
Ensure you've run `gemini` then proceed with login (via Google account) to create OAuth credentials.

### "Insecure file permissions"
Fix with:
```bash
chmod 600 ~/.gemini/oauth_creds.json
```

### "Project resolution failed"
Check your OAuth token hasn't expired:
```bash
curl http://localhost:8080/health
```

### "403 PERMISSION_DENIED"
Your OAuth token may lack necessary scopes. Re-authenticate with Gemini CLI.

## Roadmap

- [x] Phase 1: Core infrastructure with project resolution
- [ ] Phase 2: Full request/response translation
- [ ] Phase 3: Streaming support and token refresh
- [ ] Phase 4: Production readiness (metrics, tests)
- [ ] Phase 5: Release and distribution

## Contributing

Contributions welcome! Please:
1. Fork the repository
2. Create a feature branch
3. Write tests for new functionality
4. Submit a pull request

## License

MIT OR Apache-2.0

## Acknowledgments

- [Claude Code](https://github.com/anthropics/anthropic-tools) by Anthropic
- Google Gemini team for the powerful models
- Rust community for excellent libraries

---

**Author:** kelexine  
**GitHub:** https://github.com/kelexine/gem2claude
