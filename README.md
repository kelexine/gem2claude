# gem2claude

> **Use Claude Code with Google OAuth Login** — No API key billing required

[![Author](https://img.shields.io/badge/Author-kelexine-blue)](https://github.com/kelexine)
[![License](https://img.shields.io/badge/License-Apache%202.0-green)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.70+-orange)](https://www.rust-lang.org/)

A blazing-fast proxy that lets you use **Claude Code CLI** with Google's Gemini models through OAuth authentication. No API keys required, just your Google account, whether on free tier or Google AI Pro/Ultra Subscription.

## Why gem2claude?

- 🆓 **Free Tier Access** — Use Gemini through Google Cloud's OAuth, no API billing
- 📊 **Google AI Pro/Ultra Access** — Access to higher limits and Latest models
- 🚀 **Full Claude Code Support** — Streaming, tool calls, vision, extended thinking
- ⚡ **Optimized Performance** — Connection pooling, TCP keep-alive, minimal latency
- 🔒 **Secure** — OAuth with automatic token refresh, no credentials in code


## User Story (Why gem2claude?, what triggered this weird idea?)

> [!NOTE]
> **Project Goal: Gemini-to-Claude API Bridge**
> so, here's the deal, i have a Google Pro plan from last year, which as you know gives you access to google's latest models (including flagship models like the all new Gemini 3 pro/flash), so i want to use AI agents in my local terminal which both tools already provides, but the problem is.
> 1. Google's Gemini cli is not cutting it for me, and i've heard claude code is the king of terminal agents, plus it's vast tool liberary, plugins and to crown it all community support (lead by Anthropic itself).
> 2. Claude Code requires either a paid plan or api subscription which are Costly (although worth it), and since i already have a gemini pro plan buying api access or a paid plan on claude will, burn a hole in my pocket (yes am that broke) and Is kinda a waste of money since i will be paying twice.
> 
> so here's where my idea comes in, Claude code already supports routing api calls to custom endpoints, my plan: Create a tool that serves a claude code compatible api endpoint and routes the api call to Googles gemini api via OAuth (not the traditional generative api endpoint), the tool will connect to the same endpoint that Gemini Code Assist or Gemini CLI uses when authenticated via OAuth (again not the traditional generative api endpoint).

## Supported Models

| Claude Model | Gemini Backend | Best For |
|--------------|----------------|----------|
| `claude-opus-4-5` | `gemini-3-pro-preview` | Complex reasoning, analysis |
| `claude-sonnet-4-5` | `gemini-3-pro-preview` | Coding & Code Review |
| `claude-haiku-4-5` | `gemini-3-flash-preview` | Fastest responses |

## Quick Start

### 1. Build

```bash
git clone https://github.com/kelexine/gem2claude
cd gem2claude
cargo build --release
```

### 2. Get OAuth Credentials

You need OAuth credentials from [Gemini CLI](https://github.com/google-gemini/gemini-cli).

Install globally with npm:

```bash
npm install -g @google/gemini-cli@latest
```

Install globally with Homebrew (macOS/Linux):

```bash
brew install gemini-cli
```

Login & Authenticate:
```bash
gemini
```
-  Choose Login with Google
-  Choose the account you want to use
-  Follow the instructions to authenticate

After authenticating with Gemini CLI, `~/.gemini/oauth_creds.json` will automatically be created.

### 3. Run

```bash
./target/release/gem2claude
```

Proxy starts on `http://127.0.0.1:8080`

### 4. Configure Claude Code

```bash
export ANTHROPIC_BASE_URL="http://localhost:8080"
export ANTHROPIC_AUTH_TOKEN="dummy"
```

Add to `~/.bashrc` or `~/.zshrc` for persistence.

## Features

### Extended Thinking (Ultrathink)

Use Claude Code's `ultrathink` command for step-by-step reasoning:

```
❯ ultrathink: explain this codebase
```

The proxy translates Claude's thinking blocks to Gemini's native thinking feature.

### Vision Support

Analyze images directly:

```bash
claude "What's in this image? @screenshot.png"
```

Supports JPEG, PNG, WebP, GIF, HEIC up to 100MB.

### Agentic Tool Calls

Full support for Claude Code's tools:
- File read/write operations
- Bash command execution
- Browser automation (via Claude Code's browser tool)
- Multi-turn conversations with tool results

## Architecture

```
Claude Code CLI
      │
      ▼
┌─────────────────┐
│   gem2claude    │
│   (Rust Proxy)  │
├─────────────────┤
│ • Request Translation │
│ • SSE Streaming       │
│ • OAuth Management    │
│ • Thinking Support    │
└─────────────────┘
      │
      ▼
Google Gemini API

```

## Performance Optimizations

- **Connection Pooling** — 10 idle connections kept warm per host
- **TCP Keep-Alive** — 60-second intervals prevent drops
- **TCP_NODELAY** — Nagle's algorithm disabled for low latency
- **90s Idle Timeout** — Connections reused between requests
- **Minimal Logging** — Hot path optimized for speed

## Configuration

Optional environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `GEM2CLAUDE_PORT` | `8080` | Proxy port |
| `RUST_LOG` | `info` | Log level (`debug`, `info`, `warn`, `error`) |

## Troubleshooting


### Connection Issues
Check that the proxy is running and `ANTHROPIC_BASE_URL` is set correctly.

### Debug Mode
```bash
RUST_LOG=debug ./target/release/gem2claude
```

## License

Apache 2.0 — See [LICENSE](LICENSE)

## Author

**kelexine** — [GitHub](https://github.com/kelexine)
