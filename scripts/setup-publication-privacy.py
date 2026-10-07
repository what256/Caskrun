#!/usr/bin/env python3
"""Configure local-only privacy terms, anonymous authorship, and Git hooks."""
from __future__ import annotations

import json
from pathlib import Path
import socket
import subprocess

NAME = "Caskrun Contributors"
EMAIL = "contributors@caskrun.invalid"


def git(*args: str) -> str:
    return subprocess.check_output(["git", *args], text=True).strip()


def optional_config(key: str) -> str:
    result = subprocess.run(["git", "config", "--get", key], capture_output=True, text=True)
    return result.stdout.strip() if result.returncode == 0 else ""


def main() -> None:
    root = Path(git("rev-parse", "--show-toplevel"))
    if Path.cwd().resolve() != root.resolve():
        raise SystemExit("Run this script from the repository root.")
    if not (root / ".githooks" / "pre-commit").is_file():
        raise SystemExit("Tracked privacy hooks are missing.")
    local_path = Path(git("rev-parse", "--git-path", "publication-private.json"))
    if local_path.is_symlink():
        raise SystemExit("Refusing a linked local privacy configuration.")
    terms: set[str] = set()
    if local_path.exists():
        existing = json.loads(local_path.read_text())
        if not isinstance(existing, dict) or not isinstance(existing.get("terms"), list):
            raise SystemExit("Invalid local privacy configuration; review it without uploading it.")
        if not all(isinstance(term, str) for term in existing["terms"]):
            raise SystemExit("Private match terms must be strings.")
        terms.update(existing["terms"])
    current_name = optional_config("user.name")
    current_email = optional_config("user.email")
    for value in (current_name, current_email, Path.home().name, socket.gethostname()):
        if value and value.casefold() not in {NAME.casefold(), EMAIL.casefold()}:
            terms.add(value)
    local_path.write_text(json.dumps({"terms": sorted(terms)}, indent=2) + "\n")
    local_path.chmod(0o600)
    subprocess.run(["git", "config", "--local", "user.name", NAME], check=True)
    subprocess.run(["git", "config", "--local", "user.email", EMAIL], check=True)
    subprocess.run(["git", "config", "--local", "user.useConfigOnly", "true"], check=True)
    subprocess.run(["git", "config", "--local", "core.hooksPath", ".githooks"], check=True)
    print("Configured local privacy terms, neutral authorship, and commit/push hooks.")
    print("Private values were not printed or written to tracked source.")
    print("Use TZ=UTC for commits; review staged files and outgoing history.")


if __name__ == "__main__":
    main()
