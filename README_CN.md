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
  <strong>长期运行的个人 AI 助手。单个二进制文件，开箱即用。</strong>
</p>

<p align="center">
  基于 Rust 构建的模块化 Agent 框架，集成 LLM、持久记忆、技能管理、<br/>
  会话搜索和内置 Web UI。OpenAI 兼容 API，支持任意 LLM 提供商。
</p>

<p align="center">
  <a href="README.md">🇺🇸 English</a> · <a href="README_CN.md">🇨🇳 中文</a>
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

### 安装

推荐使用 **Desktop 桌面应用** —— 开箱即用，并内置自动更新。

**macOS (Apple Silicon)** —— 从[最新 Release](https://github.com/apepkuss/iron-hermes-agent/releases/latest) 下载 `.dmg`，将 **Iron Hermes** 拖入 `/Applications`，然后执行一次：

```bash
xattr -cr "/Applications/Iron Hermes.app"
```

**Linux (x86_64 AppImage)**：

```bash
curl -L -o iron-hermes.AppImage \
  https://github.com/apepkuss/iron-hermes-agent/releases/latest/download/Iron-Hermes-v<version>-x86_64-unknown-linux-gnu.AppImage
chmod +x iron-hermes.AppImage && ./iron-hermes.AppImage
```

CLI 服务端、Linux ARM64、`.deb` 包、下载校验以及各平台详细说明，请参考 **[SETUP_CN.md](SETUP_CN.md)**。

## 数据存储

所有数据存储在 `~/.iron-hermes/` 目录下：

```
~/.iron-hermes/
  config.yaml        # 配置文件
  state.db           # SQLite 数据库（会话、消息、FTS5 索引）
  memories/          # 持久化记忆文件
  skills/            # 用户创建的技能
```

## 许可证

Apache-2.0
