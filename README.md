# gem2claude

> **Use Claude Code with Google OAuth Login** — No API key billing required

[![Author](https://img.shields.io/badge/Author-kelexine-blue)](https://github.com/kelexine)
[![License](https://img.shields.io/badge/License-Apache%202.0-green)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.70+-orange)](https://www.rust-lang.org/)

A blazing-fast proxy that lets you use **Claude Code** with Google's Gemini models through OAuth authentication. No API keys required, just your Google account, whether on free tier or Google AI Pro/Ultra Subscription.

## ✨ Features

- 🆓 **Free Tier Access** — Use Gemini through Google Cloud's OAuth, no API billing
- 📊 **Google AI Pro/Ultra Access** — Access to higher limits and latest flagship models
- 🚀 **Full Claude Code Support** — Streaming, tool calls, vision, extended thinking
- ⚡ **Optimized Performance** — Connection pooling, TCP keep-alive, minimal latency
- 🧠 **Extended Thinking** — Ultrathink support with real-time streaming
- 👁️ **Vision Support** — Image analysis (JPEG, PNG, WebP, GIF, HEIC up to 100MB)
- 🔧 **Tool Execution** — File operations, bash commands, browser automation
- 💾 **Context Caching** — Reduce costs and latency with LRU-backed translation caching
- 🔒 **Secure** — OAuth with automatic token refresh, no credentials in code
- 📈 **Observable** — Prometheus metrics endpoint for monitoring cache hit rates and API usage

## Why gem2claude?

> [!NOTE]
>
> User Story
>
> So, here's the deal: I have a Google Pro plan from last year, which gives you access to Google's latest models (including flagship models like the all-new Gemini 3 Pro/Flash). I want to use AI agents in my local terminal, but the problem is:
>
> 1. Google's Gemini CLI is not cutting it for me, and I've heard Claude Code is the king of terminal agents, plus it has a vast tool library, plugins, and community support (led by Anthropic itself).
> 2. Claude Code requires either a paid plan or API subscription which are costly (although worth it), and since I already have a Gemini Pro plan, buying API access or a paid plan on Claude would burn a hole in my pocket (yes, I am that broke) and is kind of a waste of money since I'd be paying twice.
>
> So here's where my idea comes in: Claude Code already supports routing API calls to custom endpoints. My plan: Create a tool that serves a Claude Code-compatible API endpoint and routes the API call to Google's Gemini API via OAuth (not the traditional generative API endpoint). The tool connects to the same endpoint that Gemini Code Assist or Gemini CLI uses when authenticated via OAuth.

## 📋 Supported Models

| Claude Model | Gemini Backend | Context Caching | Best For |
|--------------|----------------|-----------------|----------|
| `claude-opus-4-5` | `gemini-3-pro-preview` | ✅ | Complex reasoning, analysis |
| `claude-sonnet-4-5` | `gemini-3-pro-preview` | ✅ | Coding & code review |
| `claude-haiku-4-5` | `gemini-3-flash-preview` | ✅ | Fastest responses |

## 🚀 Quick Start

### 1. Build from Source

```bash
git clone https://github.com/kelexine/gem2claude
cd gem2claude
cargo build --release
```

### 2. Get OAuth Credentials

You need OAuth credentials from [Gemini CLI](https://github.com/google-gemini/gemini-cli).

**Install Gemini CLI:**

With npm:
```bash
npm install -g @google/gemini-cli@latest
```

With Homebrew (macOS/Linux):
```bash
brew install gemini-cli
```

**Authenticate:**
```bash
gemini
```
- Choose "Login with Google"
- Select the account you want to use
- Follow the authentication flow

After authenticating, `~/.gemini/oauth_creds.json` will be created automatically.

### 3. Run the Proxy

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

## 🎯 Key Features

### Extended Thinking (Ultrathink)

gem2claude detects the **"Ultrathink" keyword** in your messages and automatically enables Gemini's highest thinking level (30k tokens):

```
❯ Ultrathink: explain this codebase architecture
```

**Features:**
- **Auto-detection**: Case-insensitive keyword scanning in user messages
- **Highest level**: Forces 30k token thinking budget
- **Remapped budgets**: LOW→15k, MEDIUM→20k, HIGH→30k tokens
- **Real-time streaming**: Thinking content streams as it's generated

**Note**: Claude Code v2.1.9+ deprecated native Ultrathink support and now uses max thinking by default. However, gem2claude's detection still works for direct API calls, older clients, and explicit user control.

The proxy translates Gemini's native thinking to Claude's thinking blocks seamlessly.

### Vision Support

Analyze images directly in your conversations:

```bash
claude "What's in this image? @screenshot.png"
```

Supports JPEG, PNG, WebP, GIF, HEIC up to 100MB. The proxy handles base64 encoding and MIME type detection automatically.

### Context Caching (NEW!)

Reduce costs by 75-90% on repeated prompts:

```bash
# Enable caching
export ENABLE_CONTEXT_CACHING=true

# First request creates cache
claude "Review this large codebase @src/**/*.rs"

# Subsequent requests hit cache (90% cost reduction)
claude "Now check for security issues"
```

Cache automatically expires after 5 minutes.

### Agentic Tool Calls

Full support for Claude Code's tool ecosystem:
- File read/write operations
- Bash command execution
- Browser automation (via Claude Code's browser tool)
- Multi-turn conversations with tool results
- Automatic thought signature management for Gemini 3.x

### Observability

Comprehensive Prometheus metrics available at `/metrics`:

- `gemini_api_calls_total`: API call counts by model and status
- `request_duration_seconds`: Latency histograms
- `translation_cache_operations_total`: Hit/miss/eviction rates for the internal translation cache
- `cache_operations_total`: Gemini context cache hit/miss/create rates

## ⚙️ Configuration

Optional environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `GEM2CLAUDE_PORT` | `8080` | Proxy server port |
| `RUST_LOG` | `info` | Log level (`debug`, `info`, `warn`, `error`) |
| `ENABLE_CONTEXT_CACHING` | `false` | Enable context caching for cost savings |

## 🏗️ Architecture

```
Claude Code CLI
      │
      ▼
┌────────────────────┐
│    gem2claude      │
│   (Rust Proxy)     │
├────────────────────┤
│ • Request Translation │
│ • SSE Streaming       │
│ • OAuth Management    │
│ • Extended Thinking   │
│ • Context Caching     │
│ • Vision Processing   │
└────────────────────┘
      │
      ▼
Google Gemini API
(cloudcode-pa.googleapis.com)
```

## ⚡ Performance Optimizations

The proxy is built for performance:

- **Connection Pooling** — 10 idle connections kept warm per host
- **TCP Keep-Alive** — 60-second intervals prevent connection drops
- **TCP_NODELAY** — Nagle's algorithm disabled for low latency
- **90s Idle Timeout** — Connections reused between requests
- **Minimal Logging** — Hot path optimized for speed
- **Immediate SSE Flushing** — Real-time streaming with keepalive comments
- **Smart Caching** — LRU in-memory translation cache to skip redundant processing
- **Deterministic Hashing** — SHA256 cache keys normalized for tool ordering and capability toggles

## 🔧 Troubleshooting

### Connection Issues

Check that the proxy is running and `ANTHROPIC_BASE_URL` is set correctly:

```bash
curl http://localhost:8080/health
```

### Debug Mode

Enable detailed logging:

```bash
RUST_LOG=debug ./target/release/gem2claude
```

### Rate Limiting

If you hit Gemini API quota limits, the proxy will return HTTP 429 with details:

```
Error: Gemini API quota exceeded: Resource exhausted (quota)
```

Wait a moment and retry, or use a different model.

## 🤝 Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## 📄 License

Apache 2.0 — See [LICENSE](LICENSE)

## 💖 Support

If you find this project useful, consider supporting its development:

- ⭐ Star this repository
- 🐛 Report issues and suggest features
- 💵 [Buy me a coffee](https://buymeacoffee.com/kelexine)
- 💰 [Sponsor on GitHub](https://github.com/sponsors/kelexine)

## 👤 Author

**kelexine** — [GitHub](https://github.com/kelexine)

## 🙏 Acknowledgments

- [Google Gemini CLI](https://github.com/google-gemini/gemini-cli) — For OAuth implementation reference
- [Anthropic Claude](https://www.anthropic.com/) — For the amazing Claude Code CLI
- The Rust community for excellent tooling and libraries

---

**Star History**

[![Star History Chart](https://api.star-history.com/svg?repos=kelexine/gem2claude&type=Date)](https://star-history.com/#kelexine/gem2claude&Date)
