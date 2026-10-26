#!/usr/bin/env python3
"""Write public update configuration and monotonic build metadata into a candidate."""
from __future__ import annotations

import base64
import os
from pathlib import Path
import plistlib
import re
import subprocess
import sys
from urllib.parse import urlsplit


def configure(info: dict, values: dict[str, str], default_build: str) -> dict:
    result = dict(info)
    version = values.get("CASKRUN_APP_VERSION", "0.2.0")
    build = values.get("CASKRUN_APP_BUILD", default_build)
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise ValueError("App version must contain three numbers")
    if not re.fullmatch(r"[1-9][0-9]{0,8}", build):
        raise ValueError("App build must be a positive increasing number")
    result.update(CFBundleShortVersionString=version, CFBundleVersion=build)
    feed = values.get("CASKRUN_UPDATE_FEED_URL", "")
    key = values.get("CASKRUN_UPDATE_PUBLIC_KEY", "")
    if bool(feed) != bool(key):
        raise ValueError("A release requires both its signed GitHub feed and public update key")
    # Avoid inheriting a different build's update channel.
    for field in ("SUFeedURL", "SUPublicEDKey"):
        result.pop(field, None)
    if feed:
        parsed = urlsplit(feed)
        parts = parsed.path.strip("/").split("/")
        if (parsed.scheme != "https" or parsed.hostname not in {"github.com", "raw.githubusercontent.com"}
                or parsed.username or parsed.password or parsed.port or parsed.query or parsed.fragment
                or "%" in parsed.path or "\\" in parsed.path or "//" in parsed.path
                or len(parts) < 4 or parts[-1] != "appcast.xml"
                or any(part in {"", ".", ".."} for part in parts)
                or not all(re.fullmatch(r"[A-Za-z0-9_.-]+", part) for part in parts[:2])):
            raise ValueError("Update feed must be a public GitHub HTTPS appcast")
        if parsed.hostname == "github.com" and not (len(parts) == 6 and parts[2] == "releases"
                and (parts[3:5] == ["latest", "download"] or parts[3] == "download")):
            raise ValueError("GitHub feeds must be attached release appcasts")
        try:
            decoded = base64.b64decode(key, validate=True)
        except ValueError as error:
            raise ValueError("Public update key is invalid") from error
        if len(decoded) != 32:
            raise ValueError("Public update key must contain 32 bytes")
        result.update(SUFeedURL=feed, SUPublicEDKey=key)
    result.update(SURequireSignedFeed=True, SUVerifyUpdateBeforeExtraction=True,
                  SUEnableSystemProfiling=False, SUAllowsAutomaticUpdates=False,
                  SUAutomaticallyUpdate=False, SUShowReleaseNotes=False)
    return result


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("Usage: configure-app-updates.py <candidate Info.plist>")
    project = Path(__file__).resolve().parent.parent
    path = Path(sys.argv[1]).resolve()
    if not path.is_relative_to((project / ".build").resolve()) or path.name != "Info.plist":
        raise SystemExit("Update metadata must belong to a candidate inside .build")
    count = subprocess.run(["git", "rev-list", "--count", "HEAD"], cwd=project,
                           capture_output=True, text=True)
    build = count.stdout.strip() if count.returncode == 0 else "1"
    try:
        with path.open("rb") as handle:
            info = plistlib.load(handle)
        if info.get("CFBundleIdentifier") != "dev.caskrun.app":
            raise ValueError("Unrecognized application")
        result = configure(info, dict(os.environ), build)
        with path.open("wb") as handle:
            plistlib.dump(result, handle, sort_keys=False)
    except (ValueError, OSError, plistlib.InvalidFileException) as error:
        raise SystemExit(str(error)) from error


if __name__ == "__main__":
    main()
