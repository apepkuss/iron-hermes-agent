# Installation Guide

<p align="center">
  <a href="SETUP.md">🇺🇸 English</a> · <a href="SETUP_CN.md">🇨🇳 中文</a>
</p>

This guide covers installation for all supported platforms and explains every file you see on the [Releases page](https://github.com/apepkuss/iron-hermes-agent/releases).

## Contents

1. [Choose Your Installation](#choose-your-installation)
2. [Desktop App](#desktop-app)
3. [CLI Server](#cli-server)
4. [Release Files Explained](#release-files-explained)
5. [Verify Downloads](#verify-downloads)
6. [Uninstall](#uninstall)
7. [Troubleshooting](#troubleshooting)

## Choose Your Installation

| Option | Who it's for | GUI | Auto-update |
|---|---|:---:|:---:|
| **Desktop App** | Most users; point-and-click local usage | ✅ | ✅ (macOS `.dmg`, Linux `.AppImage`) |
| **CLI Server** | Headless servers, scripted deployments, power users | ❌ (browser) | ❌ (manual) |

Both are built from the same codebase and run the same service — pick whichever fits your workflow.

## Desktop App

### macOS (Apple Silicon)

1. Download `Iron-Hermes-v<version>-aarch64-apple-darwin.dmg` from the [latest release](https://github.com/apepkuss/iron-hermes-agent/releases/latest).
2. Double-click the `.dmg`, drag **Iron Hermes** into `/Applications`.
3. The app is not notarized by Apple. Remove the quarantine attribute once:
   ```bash
   xattr -cr "/Applications/Iron Hermes.app"
   ```
4. Launch from Launchpad or Spotlight.

### Linux — AppImage (recommended)

The AppImage is a single portable file and is the only Linux format that supports automatic updates.

```bash
# x86_64
curl -L -o iron-hermes.AppImage \
  https://github.com/apepkuss/iron-hermes-agent/releases/latest/download/Iron-Hermes-v<version>-x86_64-unknown-linux-gnu.AppImage
chmod +x iron-hermes.AppImage
./iron-hermes.AppImage

# ARM64: replace the target with aarch64-unknown-linux-gnu
```

If you see "AppImages require FUSE to run":
```bash
sudo apt install libfuse2   # Debian/Ubuntu
```

### Linux — .deb (Debian/Ubuntu)

```bash
curl -LO https://github.com/apepkuss/iron-hermes-agent/releases/latest/download/iron-hermes-desktop-v<version>-x86_64-unknown-linux-gnu.deb
sudo dpkg -i iron-hermes-desktop-v<version>-x86_64-unknown-linux-gnu.deb
iron-hermes-desktop
```

> **Note:** `.deb` installs are managed by `apt` and cannot self-update. The in-app "Check for Updates" button will tell you so and suggest `sudo apt upgrade` or switching to the AppImage version.

### First Launch

- A default config is auto-generated at `~/.iron-hermes/config.yaml`.
- The app window loads the built-in WebUI directly — no browser needed.
- Configure your LLM provider under **Settings → Provider URL / API Key**.

See the [README Configuration section](README.md#configuration) for the full schema.

### Auto-Update

Desktop apps (macOS `.dmg` and Linux `.AppImage`) check for updates 8 seconds after startup. When a new version is available:

1. A modal shows the version diff and release notes.
2. Click **Install & Restart** — the app downloads a signed archive, verifies the minisign signature against the embedded public key, replaces itself in place, and restarts.
3. Click **Later** to postpone; you'll be prompted again next launch.

To check manually: **Settings → About → Check for Updates**.

The public key is compiled into the binary, so only releases signed by the maintainer's private key will install.

## CLI Server

The CLI ships the same service without a window. You run it and access the WebUI at `http://localhost:8080` in any browser — ideal for headless servers or background daemons.

### Download

Replace `<version>` with a tag from the [Releases page](https://github.com/apepkuss/iron-hermes-agent/releases).

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

# Linux (ARM64): replace the target with aarch64-unknown-linux-gnu
```

### Run as a Background Service

**Linux (systemd)** — `/etc/systemd/system/iron-hermes.service`:
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

**macOS (launchd)** — `~/Library/LaunchAgents/com.iron-hermes.plist`:
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

### Upgrade

The CLI does not auto-update. To upgrade, stop the running instance, download the new `.tar.gz` for your target, extract, and replace the binary.

## Release Files Explained

Each Release contains ~16 files. Most users only need one — the Desktop installer or the CLI archive. The rest are internal to the auto-update mechanism or for verification.

| File pattern | Purpose | Who needs it |
|---|---|---|
| `iron-hermes-v<V>-<target>.tar.gz` | CLI archive: headless service binary + README | CLI users |
| `Iron-Hermes-v<V>-<target>.dmg` | macOS desktop installer | macOS desktop users (first install) |
| `Iron-Hermes-v<V>-<target>.AppImage` | Linux portable desktop app | Linux desktop users (first install) |
| `iron-hermes-desktop-v<V>-<target>.deb` | Debian/Ubuntu desktop package | Linux desktop users via apt |
| `*.app.tar.gz` / `*.AppImage.tar.gz` | Updater payload (app compressed for in-place replacement) | Auto-update only — don't download manually |
| `*.app.tar.gz.sig` / `*.AppImage.tar.gz.sig` | Minisign signature of the updater payload | Auto-update only |
| `latest.json` | Updater manifest (index that tells clients which archive + signature to fetch) | Auto-update only |
| `checksums.txt` | SHA256 checksums for all artifacts | Anyone verifying downloads |

### Target names

| Target | Platform |
|---|---|
| `aarch64-apple-darwin` | macOS Apple Silicon |
| `x86_64-unknown-linux-gnu` | Linux x86_64 |
| `aarch64-unknown-linux-gnu` | Linux ARM64 |

## Verify Downloads

Every release includes `checksums.txt`. To verify a downloaded file:

```bash
curl -LO https://github.com/apepkuss/iron-hermes-agent/releases/download/v<version>/checksums.txt
sha256sum -c checksums.txt --ignore-missing
# macOS: use shasum -a 256 -c checksums.txt --ignore-missing
```

## Uninstall

| Installation | Remove the app | Remove user data |
|---|---|---|
| macOS Desktop | `rm -rf "/Applications/Iron Hermes.app"` | `rm -rf ~/.iron-hermes` |
| Linux AppImage | delete the `.AppImage` file | `rm -rf ~/.iron-hermes` |
| Linux `.deb` | `sudo apt remove iron-hermes-desktop` | `rm -rf ~/.iron-hermes` |
| CLI | delete the extracted directory | `rm -rf ~/.iron-hermes` |

User data (sessions, memories, skills, SQLite DB) lives at `~/.iron-hermes/` and is preserved unless you delete it explicitly.

## Troubleshooting

**macOS: "Iron Hermes is damaged and can't be opened"**
The app isn't notarized. Remove the quarantine attribute:
```bash
xattr -cr "/Applications/Iron Hermes.app"
```

**Linux AppImage: fails to start with FUSE error**
```bash
sudo apt install libfuse2
```

**Desktop app shows "updater unavailable"**
Expected on `.deb` installs — apt manages the install and the app cannot replace system-owned files. Use `sudo apt upgrade` or switch to the AppImage version.

**Port 8080 already in use**
Change `server.port` in `~/.iron-hermes/config.yaml` or export `IRON_PORT=8081` before launching.

**Desktop app update fails with "signature verification failed"**
The downloaded archive doesn't match the expected signature — likely a corrupted download or a network MITM. Retry; if it persists, file an issue.
