<p align="center">
  <img src="assets/logo.png" width="160" alt="iron-hermes-agent logo"/>
</p>

<h1 align="center">IRON HERMES</h1>

<p align="center">
  <img src="https://img.shields.io/badge/Built_with-Rust-B7410E?logo=rust" alt="Rust"/>
  <img src="https://img.shields.io/badge/License-MIT-blue" alt="License"/>
</p>

<p align="center">
  <strong>长期运行的个人 AI 助手。单个二进制文件，开箱即用。</strong>
</p>

<p align="center">
  基于 Rust 构建的模块化 Agent 框架，集成 LLM、持久记忆、技能管理、<br/>
  会话搜索和内置 Web UI。OpenAI 兼容 API，支持任意 LLM 提供商。
</p>

<p align="center">
  <a href="README.md">English</a> · <a href="README_CN.md">中文</a>
</p>

## 功能特性

- **Agent 循环** - 迭代式 LLM-工具执行循环，可配置迭代预算（默认 90 轮）
- **11+ 内置工具** - 文件操作、终端执行、Web 搜索、代码沙箱、记忆、会话搜索、技能等
- **持久记忆** - 跨会话知识保持，基于文件的原子存储
- **技能系统** - 90+ 内置技能，支持动态创建、编辑和管理
- **会话搜索** - 历史对话全文搜索（通过 jieba/FTS5 支持中英文）
- **上下文压缩** - 接近 token 上限时自动摘要压缩
- **Web UI** - 内置聊天界面，支持 Markdown/LaTeX 渲染
- **环境隔离** - 每个 Session 独立的工作目录和安全环境变量
- **OpenAI 兼容 API** - 直接替代 `/v1/chat/completions`
- **单一二进制** - 所有资源内嵌，SQLite 内置，零外部依赖

## 快速开始

### 前提条件

一个支持 OpenAI 兼容 API 的 LLM 提供商。推荐：

- ironmlx (推荐，Apple Silicon 平台本地 LLM 服务)
- Ollama
- vLLM
- 任何其他兼容提供商（云服务等）

### 从 Release 安装

从 [Releases](https://github.com/apepkuss/iron-hermes-agent/releases) 下载最新二进制文件：

```bash
# macOS (Apple Silicon)
curl -L https://github.com/apepkuss/iron-hermes-agent/releases/latest/download/iron-hermes-v0.1.0-aarch64-apple-darwin.tar.gz | tar xz
# 移除 macOS 隔离属性（未签名的二进制文件需要此步骤）
xattr -d com.apple.quarantine ./iron-hermes
./iron-hermes

# Linux (x86_64)
curl -L https://github.com/apepkuss/iron-hermes-agent/releases/latest/download/iron-hermes-v0.1.0-x86_64-unknown-linux-gnu.tar.gz | tar xz
./iron-hermes

# Linux (ARM64)
curl -L https://github.com/apepkuss/iron-hermes-agent/releases/latest/download/iron-hermes-v0.1.0-aarch64-unknown-linux-gnu.tar.gz | tar xz
./iron-hermes
```

### 从源码构建

```bash
git clone https://github.com/apepkuss/iron-hermes-agent.git
cd iron-hermes-agent
cargo build --release
./target/release/iron-server
```

### 运行

```bash
# 使用默认配置启动（首次运行自动生成 ~/.iron-hermes/config.yaml）
./iron-hermes

# 或通过环境变量配置
LLM_BASE_URL=http://localhost:11434 LLM_MODEL=llama3 ./iron-hermes
```

打开浏览器访问 `http://localhost:8080` 使用 Web UI。

## 配置

首次运行时会在 `~/.iron-hermes/config.yaml` 生成默认配置：

```yaml
# LLM 提供商
model: "llama3"
base_url: "http://localhost:9068"
api_key: ""

# 服务
server:
  host: "0.0.0.0"
  port: 8080

# Agent 行为
agent:
  max_turns: 90      # 单次对话最大迭代轮数
  timeout: 600        # 单次执行超时（秒）

# Session
session:
  idle_timeout: 1800  # 空闲超时（秒）

# 上下文压缩
compression:
  enabled: true
  threshold: 0.65     # 触发压缩的上下文使用率
```

环境变量优先级最高：`LLM_MODEL`、`LLM_BASE_URL`、`LLM_API_KEY`、`IRON_HOST`、`IRON_PORT`。

## 内置工具

| 工具 | 说明 |
|------|------|
| `read_file` | 读取文件内容（带行号） |
| `write_file` | 写入或覆盖文件 |
| `patch` | 替换文件中的文本 |
| `search_files` | 使用 glob 模式搜索文件 |
| `terminal` | 执行 shell 命令 |
| `web_search` | 搜索网页（需要 `TAVILY_API_KEY`） |
| `web_extract` | 提取网页内容 |
| `memory` | 跨会话持久化信息 |
| `todo` | 会话级任务列表管理 |
| `session_search` | 搜索历史对话（支持中英文） |
| `skills_list` | 列出可用技能 |
| `skill_view` | 查看技能内容 |
| `skill_manage` | 创建、编辑、删除技能 |
| `execute_code` | 在隔离沙箱中运行 Python/Shell |

## API 端点

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/v1/chat/completions` | 聊天（OpenAI 兼容，支持流式） |
| `GET` | `/v1/models` | 列出可用模型 |
| `GET` | `/health` | 健康检查 |
| `GET` | `/api/config` | 获取当前配置 |
| `POST` | `/api/config` | 运行时更新配置 |
| `GET` | `/api/sessions/search` | 搜索历史会话 |
| `POST` | `/api/session/reset` | 重置当前会话 |
| `GET` | `/api/toolsets` | 列出已注册工具集 |

## 数据存储

所有数据存储在 `~/.iron-hermes/` 目录下：

```
~/.iron-hermes/
  config.yaml        # 配置文件
  state.db           # SQLite 数据库（会话、消息、FTS5 索引）
  memories/          # 持久化记忆文件
  skills/            # 用户创建的技能
```

## 架构

iron-hermes 以 Rust workspace 组织，包含 7 个 crate：

```
iron-hermes-agent/
  crates/
    iron-tool-api/   # 工具注册表和模块 trait
    iron-core/       # Agent 运行时、会话、搜索、压缩
    iron-memory/     # 持久化记忆管理
    iron-skills/     # 技能发现、加载、执行
    iron-tools/      # 文件、终端、Web 工具
    iron-sandbox/    # 代码执行沙箱
    iron-server/     # HTTP 服务（Axum）和 Web UI
  web/               # Web UI（编译时嵌入）
  skills/            # 内置技能（90+）
```

## 许可证

MIT
