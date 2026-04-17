# 安装指南

<p align="center">
  <a href="SETUP.md">🇺🇸 English</a> · <a href="SETUP_CN.md">🇨🇳 中文</a>
</p>

本指南介绍 Iron Hermes 在各平台的安装方式，并解释 [Releases 页面](https://github.com/apepkuss/iron-hermes-agent/releases) 中每个文件的用途。

## 目录

1. [选择安装方式](#选择安装方式)
2. [Desktop 桌面应用](#desktop-桌面应用)
3. [CLI 服务端](#cli-服务端)
4. [Release 文件说明](#release-文件说明)
5. [校验下载文件](#校验下载文件)
6. [卸载](#卸载)
7. [常见问题](#常见问题)

## 选择安装方式

| 类型 | 适合人群 | 图形界面 | 自动更新 |
|---|---|:---:|:---:|
| **Desktop 桌面应用** | 大多数用户；桌面开箱即用 | ✅ | ✅（macOS `.dmg`、Linux `.AppImage`） |
| **CLI 服务端** | 无头服务器、脚本化部署、进阶用户 | ❌（浏览器访问） | ❌（需手动升级） |

两种方式构建自同一份代码，运行同一个服务 —— 按你的使用场景选择即可。

## Desktop 桌面应用

### macOS (Apple Silicon)

1. 从[最新 Release](https://github.com/apepkuss/iron-hermes-agent/releases/latest) 下载 `Iron-Hermes-v<version>-aarch64-apple-darwin.dmg`。
2. 双击打开 `.dmg`，将 **Iron Hermes** 拖入 `/Applications`（即 Finder 里的"应用程序"）。
3. 应用未经 Apple 公证，首次启动前执行一次：
   ```bash
   xattr -cr "/Applications/Iron Hermes.app"
   ```
   否则可能提示"已损坏，无法打开"。
4. 从启动台或 Spotlight 启动应用。

### Linux — AppImage（推荐）

AppImage 是单文件便携格式，也是 Linux 上**唯一支持自动更新**的分发方式。

```bash
# x86_64
curl -L -o iron-hermes.AppImage \
  https://github.com/apepkuss/iron-hermes-agent/releases/latest/download/Iron-Hermes-v<version>-x86_64-unknown-linux-gnu.AppImage
chmod +x iron-hermes.AppImage
./iron-hermes.AppImage

# ARM64：将 target 替换为 aarch64-unknown-linux-gnu
```

若提示 "AppImages require FUSE to run"：
```bash
sudo apt install libfuse2   # Debian / Ubuntu
```

### Linux — .deb（Debian / Ubuntu）

```bash
curl -LO https://github.com/apepkuss/iron-hermes-agent/releases/latest/download/iron-hermes-desktop-v<version>-x86_64-unknown-linux-gnu.deb
sudo dpkg -i iron-hermes-desktop-v<version>-x86_64-unknown-linux-gnu.deb
iron-hermes-desktop
```

> **注意**：`.deb` 由 `apt` 管理，**不支持应用内自动更新**。点击应用中的"Check for Updates"会提示你通过 `sudo apt upgrade` 升级，或改用 AppImage 版本。

### 首次启动

- 默认配置会自动生成在 `~/.iron-hermes/config.yaml`
- 应用窗口直接加载内置 WebUI，无需打开浏览器
- 在 **Settings → Provider URL / API Key** 配置 LLM 服务端

完整配置项参考 [README 的配置章节](README_CN.md#配置)。

### 自动更新

桌面版（macOS `.dmg`、Linux `.AppImage`）会在启动 8 秒后后台检查新版本。有更新时：

1. 弹窗显示版本差异和 Release 说明
2. 点击 **Install & Restart** —— 应用会下载已签名的更新包、用内嵌公钥验证 minisign 签名、原地替换自身、然后重启
3. 点击 **Later** 暂不更新，下次启动会再次提示

也可通过 **Settings → About → Check for Updates** 手动检查。

公钥在编译时嵌入二进制，只有维护者的私钥签名的 Release 才会被接受，防止中间人投毒。

## CLI 服务端

CLI 版本不含窗口，只起 HTTP 服务；用任意浏览器访问 `http://localhost:9069` 即可。适合无头服务器或作为后台常驻服务。

### 下载

将下面命令中的 `<version>` 替换为 [Releases 页面](https://github.com/apepkuss/iron-hermes-agent/releases) 上的版本号。

```bash
# macOS (Apple Silicon)
curl -L https://github.com/apepkuss/iron-hermes-agent/releases/latest/download/iron-hermes-v<version>-aarch64-apple-darwin.tar.gz | tar xz
cd iron-hermes-v<version>-aarch64-apple-darwin
xattr -d com.apple.quarantine ./iron-hermes 2>/dev/null || true
./iron-hermes

# Linux (x86_64)
curl -L https://github.com/apepkuss/iron-hermes-agent/releases/latest/download/iron-hermes-v<version>-x86_64-unknown-linux-gnu.tar.gz | tar xz
cd iron-hermes-v<version>-x86_64-unknown-linux-gnu
./iron-hermes

# Linux (ARM64)：将 target 替换为 aarch64-unknown-linux-gnu
```

### 作为后台服务运行

**Linux (systemd)** —— `/etc/systemd/system/iron-hermes.service`：
```ini
[Unit]
Description=Iron Hermes Agent
After=network.target

[Service]
ExecStart=/usr/local/bin/iron-hermes
Restart=on-failure
User=youruser

[Install]
WantedBy=default.target
```
```bash
sudo systemctl enable --now iron-hermes
```

**macOS (launchd)** —— `~/Library/LaunchAgents/com.iron-hermes.plist`：
```xml
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
  <key>Label</key><string>com.iron-hermes</string>
  <key>ProgramArguments</key>
  <array><string>/usr/local/bin/iron-hermes</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
</dict>
</plist>
```
```bash
launchctl load ~/Library/LaunchAgents/com.iron-hermes.plist
```

### 升级

CLI 不支持自动更新。升级时：停止当前实例 → 下载对应 target 的新 `.tar.gz` → 解压替换二进制 → 重启。

## Release 文件说明

每个 Release 大约包含 16 个文件，绝大多数用户只需要**一个** —— 你选择的安装包。其余文件用于自动更新或完整性校验。

| 文件模式 | 用途 | 谁需要它 |
|---|---|---|
| `iron-hermes-v<V>-<target>.tar.gz` | CLI 归档：服务端二进制 + README | CLI 用户 |
| `Iron-Hermes-v<V>-<target>.dmg` | macOS 桌面安装包 | macOS 桌面用户（首次安装） |
| `Iron-Hermes-v<V>-<target>.AppImage` | Linux 便携桌面应用 | Linux 桌面用户（首次安装） |
| `iron-hermes-desktop-v<V>-<target>.deb` | Debian/Ubuntu 桌面包 | 通过 apt 安装的 Linux 用户 |
| `*.app.tar.gz` / `*.AppImage.tar.gz` | 更新包载荷（压缩后的应用，用于原地替换） | 仅自动更新使用，不必手动下载 |
| `*.app.tar.gz.sig` / `*.AppImage.tar.gz.sig` | 更新包的 Minisign 签名 | 仅自动更新使用 |
| `latest.json` | 更新清单（告知客户端去哪下哪个包 + 对应签名） | 仅自动更新使用 |
| `checksums.txt` | 所有文件的 SHA256 校验值 | 需要校验下载完整性的用户 |

### Target 说明

| Target | 对应平台 |
|---|---|
| `aarch64-apple-darwin` | macOS Apple Silicon |
| `x86_64-unknown-linux-gnu` | Linux x86_64 |
| `aarch64-unknown-linux-gnu` | Linux ARM64 |

## 校验下载文件

每个 Release 都附带 `checksums.txt`：

```bash
curl -LO https://github.com/apepkuss/iron-hermes-agent/releases/download/v<version>/checksums.txt
sha256sum -c checksums.txt --ignore-missing
# macOS 使用：shasum -a 256 -c checksums.txt --ignore-missing
```

## 卸载

| 安装方式 | 卸载应用 | 清理用户数据 |
|---|---|---|
| macOS Desktop | `rm -rf "/Applications/Iron Hermes.app"` | `rm -rf ~/.iron-hermes` |
| Linux AppImage | 删除 `.AppImage` 文件 | `rm -rf ~/.iron-hermes` |
| Linux `.deb` | `sudo apt remove iron-hermes-desktop` | `rm -rf ~/.iron-hermes` |
| CLI | 删除解压后的目录 | `rm -rf ~/.iron-hermes` |

用户数据（会话记录、memory、skill、SQLite 数据库）存放在 `~/.iron-hermes/`，卸载应用默认**不会**删除，需手动清理。

## 常见问题

**macOS 提示 "Iron Hermes 已损坏，无法打开"**
应用未经 Apple 公证。执行一次：
```bash
xattr -cr "/Applications/Iron Hermes.app"
```

**Linux AppImage 启动失败，提示 FUSE 错误**
```bash
sudo apt install libfuse2
```

**桌面应用显示 "updater unavailable"**
`.deb` 用户会看到此提示 —— 系统包管理器管理的安装路径应用无法自己替换。请使用 `sudo apt upgrade`，或改用 AppImage 版本以支持应用内更新。

**9069 端口被占用**
修改 `~/.iron-hermes/config.yaml` 中的 `server.port`，或启动前设置环境变量 `IRON_PORT=<新端口>`。

**桌面应用更新时提示 "signature verification failed"**
下载的更新包签名校验失败 —— 可能是下载不完整或网络中间人。重试即可；若持续出现，请提交 issue。
