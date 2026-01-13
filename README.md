# gem2claude

**OAuth-based Gemini API to Claude Code Compatible Proxy**

*Author: kelexine (https://github.com/kelexine)*

## Overview

`gem2claude` is a high-performance Rust proxy that enables Claude Code to work seamlessly with Google's Gemini models via OAuth authentication. If you have a Google Pro subscription with access to Gemini models but want to use Claude Code's superior terminal agent capabilities, this proxy is your solution.

## Current Status

**Partial** - Full streaming support with SSE translation (WIP)

- OAuth credential loading from Gemini CLI
- Project ID resolution via internal API
- Request/Response translation (Anthropic ↔ Gemini)
- Tool schema sanitization for Gemini compatibility
- SSE streaming with proper Anthropic event format (functional but needs work)
- Thinking tag stripping (`<think>` blocks filtered)

## Key Features

- **OAuth Integration** - Uses your existing Gemini CLI OAuth credentials
- **Internal API Support** - Connects to Google's Cloud Code API (`cloudcode-pa.googleapis.com`)
- **Full API Translation** - Converts between Anthropic and Gemini API formats
- **SSE Streaming** - Real-time streaming with proper event format (WIP)
- **Tool Support** - Claude Code tools translated to Gemini function calls

## Prerequisites

Before using gem2claude, you need:

1. **Google Account** with access to Gemini Cli
2. **Gemini CLI** installed and authenticated via OAuth:
   ```bash
   # Install Gemini CLI (via npm)
   npm install -g @google/gemini-cli
   
   # Or install via other methods:
   brew install gemini-cli
   
   # Then authenticate
   gemini

   ```
> NOTE: Choose `Login` as your authentication method
   
   This creates `~/.gemini/oauth_creds.json` which gem2claude uses.

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/kelexine/gem2claude
cd gem2claude

# Build release binary
cargo build --release

# The binary is at target/release/gem2claude
```

### Install Globally (Optional)

```bash
# Copy to local bin
cp target/release/gem2claude ~/.local/bin/

# Or install system-wide
sudo cp target/release/gem2claude /usr/local/bin/
```

## Quick Start

### 1. Verify OAuth credentials exist

```bash
ls -la ~/.gemini/oauth_creds.json
# Should show: -rw------- (600 permissions)
```

### 2. Start the proxy

```bash
./target/release/gem2claude
```

You should see:
```
INFO  Starting gem2claude v0.1.0
INFO  Loading OAuth credentials from ~/.gemini/oauth_creds.json
INFO  Resolving Gemini Cloud Code project ID...
INFO  Project ID resolved: parabolic-vector-xxxxx
INFO  Starting server on 127.0.0.1:8080
```

### 3. Configure Claude Code

In a new terminal, set the environment variables:

```bash
export ANTHROPIC_BASE_URL="http://localhost:8080"
export ANTHROPIC_AUTH_TOKEN="dummy"
```

### 4. Use Claude Code

```bash
claude "Write a Python script to analyze CSV files"
```

## Testing the Proxy

### Health Check

```bash
curl http://localhost:8080/health | jq '.'
```

### Test Non-Streaming

```bash
curl -X POST http://localhost:8080/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: dummy" \
  -d '{
    "model": "claude-sonnet-4",
    "max_tokens": 1000,
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### Test Streaming

```bash
curl -N -X POST http://localhost:8080/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: dummy" \
  -d '{
    "model": "claude-sonnet-4",
    "max_tokens": 1000,
    "stream": true,
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## Supported Models

gem2claude automatically maps Claude models to Gemini equivalents:

| Claude Model | Gemini Model |
|-------------|--------------|
| claude-opus-4 | gemini-3-pro-preview |
| claude-sonnet-4 | gemini-3-pro-preview |
| claude-sonnet-4-5 | gemini-3-pro-preview |
| claude-haiku-4 | gemini-43flash-preview |

## Architecture

```
Claude Code CLI
      │
      ▼
┌─────────────────────────────────────┐
│       gem2claude (localhost:8080)   │
│  ┌─────────────────────────────┐   │
│  │  Anthropic → Gemini         │   │
│  │  Request Translation        │   │
│  │  • Model mapping            │   │
│  │  • Tool schema sanitization │   │
│  │  • System prompt handling   │   │
│  └─────────────────────────────┘   │
│  ┌─────────────────────────────┐   │
│  │  Gemini → Anthropic         │   │
│  │  Response Translation       │   │
│  │  • SSE event generation     │   │
│  │  • Thinking tag stripping   │   │
│  │  • Usage metadata           │   │
│  └─────────────────────────────┘   │
└─────────────────────────────────────┘
      │
      ▼
Google Cloud Code API
(cloudcode-pa.googleapis.com/v1internal)
```

## Troubleshooting

### "Credentials file not found"

Ensure you've run `gemini` and completed the OAuth login.

### Server crashes on startup

Check if another gem2claude instance is running:
```bash
pkill gem2claude
```

## Development

```bash
# Run with debug logging
RUST_LOG=debug cargo run

# Run tests
cargo test

# Format code
cargo fmt

# Lint
cargo clippy
```

## Roadmap

- [x] **Phase 1**: Core infrastructure (OAuth, project resolution, health checks)
- [x] **Phase 2**: Full request/response translation engine
- [x] **Phase 3**: SSE streaming support
- [ ] **Phase 4**: Production hardening (retry logic, metrics, rate limit handling)
- [ ] **Phase 5**: Release (pre-built binaries, documentation)

## License

MIT OR Apache-2.0

## Acknowledgments

- Google Gemini team for the powerful models
- Anthropic for Claude Code CLI inspiration
- Rust community for excellent libraries

---

**Author:** [kelexine](https://github.com/kelexine)
**GitHub:** https://github.com/kelexine/gem2claude
