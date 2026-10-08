#!/usr/bin/env python3
"""Validate original browser-link metadata; never fetch or execute a game."""
import json
import re
import sys
from pathlib import Path
from urllib.parse import unquote, urlsplit

CATALOG = Path(__file__).with_name("browser-games.json")
FIELDS = {"id", "name", "detail", "url", "sourceURL", "symbol", "requiresOwnFiles"}


def checked_url(value):
    if not isinstance(value, str) or not 1 <= len(value.encode("utf-8")) <= 2048:
        raise ValueError("Invalid browser URL length")
    if not value.startswith("https://") or any(ord(c) <= 32 or ord(c) >= 127 or c in '\\"<>`^{}|' for c in value):
        raise ValueError("Browser URLs must be encoded HTTPS links")
    parsed = urlsplit(value)
    host = parsed.netloc.lower()
    if not host or len(host) > 253 or any(c in host for c in "@:%[]"):
        raise ValueError("Browser URLs require a plain public website name")
    labels = host.split(".")
    if len(labels) < 2 or any(not re.fullmatch(r"[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?", label) for label in labels):
        raise ValueError("Invalid website name")
    if not re.fullmatch(r"[a-z]{2,63}", labels[-1]) or labels[-1] in {"localhost", "local", "internal", "lan", "home", "invalid", "test", "example", "onion"}:
        raise ValueError("Local or reserved website name")
    for index, character in enumerate(value):
        if character == "%":
            escape = value[index + 1:index + 3]
            if not re.fullmatch(r"[0-9a-fA-F]{2}", escape) or int(escape, 16) < 32 or int(escape, 16) in (127, 92):
                raise ValueError("Invalid or hidden URL escape")
    for parameters in re.split(r"[?#]", value)[1:]:
        for item in re.split(r"[&;]", parameters):
            key = unquote(item.split("=", 1)[0]).split("[", 1)[0]
            normalized = re.sub(r"[^a-z0-9]", "", key.lower())
            if normalized in {"token", "accesstoken", "idtoken", "refreshtoken", "auth", "authtoken", "bearer", "bearertoken", "authorization", "authorizationcode", "code", "clientsecret", "jwt", "oauthtoken", "oauthtokensecret", "oauthverifier", "session", "sessionid", "sid", "apikey", "apitoken", "key", "password", "passwd", "pwd", "email"}:
                raise ValueError("Account-bearing links cannot be browser game bookmarks")
    return value


def validate(data):
    if not isinstance(data, dict) or set(data) != {"schemaVersion", "entries"} or type(data["schemaVersion"]) is not int or data["schemaVersion"] != 1:
        raise ValueError("Unsupported browser catalog schema")
    entries = data["entries"]
    if not isinstance(entries, list) or not 1 <= len(entries) <= 64:
        raise ValueError("Browser catalog must contain 1–64 entries")
    ids, urls = set(), set()
    for item in entries:
        if not isinstance(item, dict) or set(item) != FIELDS:
            raise ValueError("Unknown or missing browser game fields")
        for field, limit in (("id", 64), ("name", 160), ("detail", 512), ("symbol", 64)):
            value = item[field]
            if not isinstance(value, str) or not 1 <= len(value.encode("utf-8")) <= limit or any(ord(c) < 32 or ord(c) == 127 for c in value):
                raise ValueError(f"Invalid browser game {field}")
        if not re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)*", item["id"]) or item["id"] in ids:
            raise ValueError("Duplicate or invalid browser game ID")
        ids.add(item["id"])
        checked_url(item["url"])
        checked_url(item["sourceURL"])
        if item["url"] in urls or type(item["requiresOwnFiles"]) is not bool:
            raise ValueError("Duplicate browser game URL or invalid own-files flag")
        urls.add(item["url"])
    return len(entries)


def main():
    if len(sys.argv) > 2:
        raise ValueError("usage: validate_browser_games.py [catalog.json]")
    path = Path(sys.argv[1]) if len(sys.argv) == 2 else CATALOG
    if path.stat().st_size > 128 * 1024:
        raise ValueError("Browser catalog exceeds the size limit")
    count = validate(json.loads(path.read_text(encoding="utf-8")))
    print(f"Valid browser game catalog: {count} checked links; gameplay unverified")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError) as error:
        print(f"Invalid browser game catalog: {error}", file=sys.stderr)
        sys.exit(1)
