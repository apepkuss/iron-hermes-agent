<p align="center">
  <img src="assets/logo.png" width="160" alt="iron-hermes-agent logo"/>
</p>

<h1 align="center">IRON HERMES</h1>

<p align="center">
  <img src="https://img.shields.io/badge/Built_with-Rust-B7410E?logo=rust" alt="Rust"/>
  <img src="https://img.shields.io/badge/License-MIT-blue" alt="License"/>
</p>

<p align="center">
  <strong>A long-running personal AI assistant. Single binary, batteries included.</strong>
</p>

<p align="center">
  A modular agent framework built in Rust with LLM integration, persistent memory, skill management,<br/>
  session search, and a built-in web UI. OpenAI-compatible API, works with any LLM provider.
</p>

<p align="center">
  <a href="README.md">English</a> · <a href="README_CN.md">中文</a>
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

### Install from Release

Download the latest binary from [Releases](https://github.com/apepkuss/iron-hermes-agent/releases):

```bash
# macOS (Apple Silicon)
curl -L https://github.com/apepkuss/iron-hermes-agent/releases/latest/download/iron-hermes-v0.1.0-aarch64-apple-darwin.tar.gz | tar xz
# Remove macOS quarantine attribute (required for unsigned binaries)
xattr -d com.apple.quarantine ./iron-hermes
./iron-hermes

# Linux (x86_64)
curl -L https://github.com/apepkuss/iron-hermes-agent/releases/latest/download/iron-hermes-v0.1.0-x86_64-unknown-linux-gnu.tar.gz | tar xz
./iron-hermes

# Linux (ARM64)
curl -L https://github.com/apepkuss/iron-hermes-agent/releases/latest/download/iron-hermes-v0.1.0-aarch64-unknown-linux-gnu.tar.gz | tar xz
./iron-hermes
```

### Build from Source

```bash
git clone https://github.com/apepkuss/iron-hermes-agent.git
cd iron-hermes-agent
cargo build --release
./target/release/iron-server
```

### Run

```bash
# Start with default config (auto-generated at ~/.iron-hermes/config.yaml)
./iron-hermes

# Or configure via environment variables
LLM_BASE_URL=http://localhost:11434 LLM_MODEL=llama3 ./iron-hermes
```

Open your browser at `http://localhost:8080` to access the web UI.

## Configuration

On first run, a default config is generated at `~/.iron-hermes/config.yaml`:

```yaml
# LLM provider
model: "llama3"
base_url: "http://localhost:9068"
api_key: ""

# Server
server:
  host: "0.0.0.0"
  port: 8080

# Agent behavior
agent:
  max_turns: 90
  timeout: 600

# Session
session:
  idle_timeout: 1800

# Context compression
compression:
  enabled: true
  threshold: 0.65
```

Environment variables take highest priority: `LLM_MODEL`, `LLM_BASE_URL`, `LLM_API_KEY`, `IRON_HOST`, `IRON_PORT`.

## Built-in Tools

| Tool | Description |
|------|-------------|
| `read_file` | Read file contents with line numbers |
| `write_file` | Write or overwrite file content |
| `patch` | Replace text in files |
| `search_files` | Search files with glob patterns |
| `terminal` | Execute shell commands |
| `web_search` | Search the web (requires `TAVILY_API_KEY`) |
| `web_extract` | Extract content from web pages |
| `memory` | Persist information across sessions |
| `todo` | Per-session task list management |
| `session_search` | Search past conversations (Chinese + English) |
| `skills_list` | List available skills |
| `skill_view` | View skill content |
| `skill_manage` | Create, edit, or delete skills |
| `execute_code` | Run Python/Shell in isolated sandbox |

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/v1/chat/completions` | Chat (OpenAI-compatible, streaming supported) |
| `GET` | `/v1/models` | List available models |
| `GET` | `/health` | Health check |
| `GET` | `/api/config` | Get current configuration |
| `POST` | `/api/config` | Update configuration at runtime |
| `GET` | `/api/sessions/search` | Search past sessions |
| `POST` | `/api/session/reset` | Reset current session |
| `GET` | `/api/toolsets` | List registered toolsets |

## Data Storage

All data is stored under `~/.iron-hermes/`:

```
~/.iron-hermes/
  config.yaml        # Configuration file
  state.db           # SQLite database (sessions, messages, FTS5 index)
  memories/          # Persistent memory files
  skills/            # User-created skills
```

## Architecture

iron-hermes is organized as a Rust workspace with 7 crates:

```
iron-hermes-agent/
  crates/
    iron-tool-api/   # Tool registry and module trait
    iron-core/       # Agent runtime, session, search, compression
    iron-memory/     # Persistent memory management
    iron-skills/     # Skill discovery, loading, execution
    iron-tools/      # File, terminal, web tools
    iron-sandbox/    # Code execution sandbox
    iron-server/     # HTTP server (Axum) and web UI
  web/               # Web UI (embedded at compile time)
  skills/            # Bundled skills (90+)
```

## License

MIT
