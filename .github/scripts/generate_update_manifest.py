#!/usr/bin/env python3
"""Generate Tauri updater manifest (latest.json) from release artifacts.

Scans a directory of release artifacts for `*.tar.gz.sig` updater signatures,
pairs each with its archive, and produces a `latest.json` manifest that the
Tauri updater can consume.

Artifact filename convention (set by release.yml `Collect desktop artifacts`):
    Iron-Hermes-<version>-<target>.app.tar.gz(.sig)       # macOS
    Iron-Hermes-<version>-<target>.AppImage.tar.gz(.sig)  # Linux AppImage
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

# CI matrix target → Tauri updater platform key
TARGET_TO_PLATFORM = {
    "aarch64-apple-darwin": "darwin-aarch64",
    "x86_64-apple-darwin": "darwin-x86_64",
    "x86_64-unknown-linux-gnu": "linux-x86_64",
    "aarch64-unknown-linux-gnu": "linux-aarch64",
}

ARCHIVE_RE = re.compile(
    r"^Iron-Hermes-(?P<ver>v[^/]+?)-(?P<target>[a-zA-Z0-9_]+-[a-zA-Z0-9_-]+)"
    r"\.(?P<kind>app\.tar\.gz|AppImage\.tar\.gz)$"
)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True, help="Release tag, e.g. v0.1.1")
    parser.add_argument("--artifacts-dir", required=True)
    parser.add_argument("--repo", required=True, help="GitHub owner/name")
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    version_clean = args.version.lstrip("v")
    artifacts_dir = Path(args.artifacts_dir)
    release_url_base = (
        f"https://github.com/{args.repo}/releases/download/{args.version}"
    )

    platforms: dict[str, dict[str, str]] = {}

    for sig_path in sorted(artifacts_dir.glob("*.tar.gz.sig")):
        archive_name = sig_path.name[:-4]  # strip ".sig"
        archive_path = artifacts_dir / archive_name
        if not archive_path.exists():
            print(f"[skip] signature without archive: {sig_path.name}", file=sys.stderr)
            continue

        m = ARCHIVE_RE.match(archive_name)
        if not m:
            print(f"[skip] cannot parse: {archive_name}", file=sys.stderr)
            continue

        target = m.group("target")
        platform_key = TARGET_TO_PLATFORM.get(target)
        if platform_key is None:
            print(f"[skip] unknown target: {target}", file=sys.stderr)
            continue

        signature = sig_path.read_text().strip()
        platforms[platform_key] = {
            "signature": signature,
            "url": f"{release_url_base}/{archive_name}",
        }

    if not platforms:
        print("No updater artifacts found; aborting.", file=sys.stderr)
        return 1

    manifest = {
        "version": version_clean,
        "notes": (
            f"See release notes at "
            f"https://github.com/{args.repo}/releases/tag/{args.version}"
        ),
        "pub_date": datetime.now(timezone.utc)
        .isoformat(timespec="seconds")
        .replace("+00:00", "Z"),
        "platforms": platforms,
    }

    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"Wrote {output} with platforms: {list(platforms.keys())}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
