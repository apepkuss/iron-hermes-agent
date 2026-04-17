<p align="center">
  <img src="assets/logo.png" width="160" alt="iron-hermes-agent logo"/>
</p>

<h1 align="center">IRON HERMES</h1>

<p align="center">
  <img src="https://img.shields.io/badge/Built_with-Rust-B7410E?logo=rust" alt="Rust"/>
  <img src="https://img.shields.io/badge/Platform-Apple_Silicon-000?logo=apple" alt="macOS"/>
  <img src="https://img.shields.io/badge/Platform-Linux-333?logo=linux&logoColor=white" alt="Linux"/>
  <img src="https://img.shields.io/badge/License-Apache_2.0-blue" alt="License"/>
</p>

<p align="center">
  <strong>A long-running personal AI assistant. Single binary, batteries included.</strong>
</p>

<p align="center">
  A modular agent framework built in Rust with LLM integration, persistent memory, skill management,<br/>
  session search, and a built-in web UI. OpenAI-compatible API, works with any LLM provider.
</p>

<p align="center">
  <a href="README.md">🇺🇸 English</a> · <a href="README_CN.md">🇨🇳 中文</a>
</p>

## Features

- **Agent Loop** - Iterative LLM-tool execution cycle with configurable budget (default 90 turns)
- **11+ Built-in Tools** - File operations, terminal execution, web search, code sandbox, memory, session search, skills, and more
- **Persistent Memory** - Cross-session knowledge retention with atomic file-based storage
- **Skill System** - 90+ bundled skills with dynamic creation, editing, and management
- **Session Search** - Full-text search across conversation history (Chinese + English via jieba/FTS5)
- **Context Compression** - Automatic summarization when approaching token limits
- **Web UI** - Built-in chat interface with markdown/LaTeX rendering
- **Environment Isolation** - Per-session working directory and safe environment variables
- **OpenAI-Compatible API** - Drop-in replacement for `/v1/chat/completions`
- **Single Binary** - All assets embedded, SQLite bundled, zero external dependencies

## Quick Start

### Prerequisites

An LLM provider with OpenAI-compatible API. Recommended:

- ironmlx (recommended, local LLM serving on Apple Silicon)
- Ollama
- vLLM
- Any other compatible provider (cloud providers, etc.)

### Install

The **Desktop app** is the recommended way to get started — single-click install with built-in auto-updates.

**macOS (Apple Silicon)** — download the `.dmg` from the [latest release](https://github.com/apepkuss/iron-hermes-agent/releases/latest), drag **Iron Hermes** into `/Applications`, then run once:

```bash
xattr -cr "/Applications/Iron Hermes.app"
```

**Linux (x86_64 AppImage)**:

```bash
curl -L -o iron-hermes.AppImage \
  https://github.com/apepkuss/iron-hermes-agent/releases/latest/download/Iron-Hermes-v<version>-x86_64-unknown-linux-gnu.AppImage
chmod +x iron-hermes.AppImage && ./iron-hermes.AppImage
```

For the CLI server, Linux ARM64, `.deb` package, download verification, and detailed per-platform instructions, see **[SETUP.md](SETUP.md)**.

## Data Storage

All data is stored under `~/.iron-hermes/`:

```
~/.iron-hermes/
  config.yaml        # Configuration file
  state.db           # SQLite database (sessions, messages, FTS5 index)
  memories/          # Persistent memory files
  skills/            # User-created skills
```

## License

Apache-2.0
