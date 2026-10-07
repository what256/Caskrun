#!/usr/bin/env python3
"""Validate the bundled catalog against its small, dependency-free schema subset."""

from __future__ import annotations

import ipaddress
import json
import re
import sys
from pathlib import Path
from urllib.parse import urlsplit


CATALOG_DIR = Path(__file__).resolve().parent
SCHEMA_PATH = CATALOG_DIR / "schema.json"
DEFAULT_CATALOG_PATH = CATALOG_DIR / "apps.json"


class CatalogError(ValueError):
    pass


def validate_url(value: str, location: str) -> None:
    if any(character.isspace() or ord(character) < 32 for character in value):
        raise CatalogError(f"{location}: URL contains whitespace or a control character")
    try:
        parts = urlsplit(value)
        host = parts.hostname
        port = parts.port
    except ValueError as error:
        raise CatalogError(f"{location}: malformed URL: {error}") from error
    if (
        parts.scheme != "https"
        or not host
        or "." not in host
        or parts.username is not None
        or parts.password is not None
        or port is not None
        or parts.fragment
    ):
        raise CatalogError(f"{location}: expected a public HTTPS page URL without credentials, port, or fragment")
    try:
        ipaddress.ip_address(host)
    except ValueError:
        pass
    else:
        raise CatalogError(f"{location}: IP-address URLs are not allowed")


def resolve_ref(schema: dict, reference: str) -> dict:
    if not reference.startswith("#/"):
        raise CatalogError(f"schema: unsupported reference {reference!r}")
    result = schema
    for segment in reference[2:].split("/"):
        result = result[segment.replace("~1", "/").replace("~0", "~")]
    return result


def validate_value(value: object, rule: dict, schema: dict, location: str) -> None:
    if "$ref" in rule:
        validate_value(value, resolve_ref(schema, rule["$ref"]), schema, location)
        return
    if "const" in rule and (type(value) is not type(rule["const"]) or value != rule["const"]):
        raise CatalogError(f"{location}: expected {rule['const']!r}")
    if "enum" in rule and value not in rule["enum"]:
        raise CatalogError(f"{location}: expected one of {rule['enum']!r}")

    kind = rule.get("type")
    if kind == "object":
        if not isinstance(value, dict):
            raise CatalogError(f"{location}: expected an object")
        missing = set(rule.get("required", [])) - value.keys()
        if missing:
            raise CatalogError(f"{location}: missing {', '.join(sorted(missing))}")
        properties = rule.get("properties", {})
        if rule.get("additionalProperties") is False:
            extra = value.keys() - properties.keys()
            if extra:
                raise CatalogError(f"{location}: unknown fields: {', '.join(sorted(extra))}")
        for key, field_value in value.items():
            if key in properties:
                validate_value(field_value, properties[key], schema, f"{location}.{key}")
    elif kind == "array":
        if not isinstance(value, list):
            raise CatalogError(f"{location}: expected an array")
        if len(value) < rule.get("minItems", 0):
            raise CatalogError(f"{location}: array is too short")
        for index, item in enumerate(value):
            validate_value(item, rule["items"], schema, f"{location}[{index}]")
    elif kind == "string":
        if not isinstance(value, str):
            raise CatalogError(f"{location}: expected a string")
        if len(value.strip()) < rule.get("minLength", 0):
            raise CatalogError(f"{location}: text is empty")
        if "pattern" in rule and re.search(rule["pattern"], value) is None:
            raise CatalogError(f"{location}: does not match {rule['pattern']!r}")
        if rule.get("format") == "uri":
            validate_url(value, location)
    elif kind is not None:
        raise CatalogError(f"schema: unsupported type {kind!r}")


def validate_catalog(data: object, schema: dict) -> int:
    validate_value(data, schema, schema, "catalog")
    assert isinstance(data, dict)
    entries = data["entries"]
    for entry in entries:
        if re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)*", entry["id"]) is None:
            raise CatalogError(f"catalog.entries: invalid ID {entry['id']!r}")
        if entry.get("installMode") == "automatic" and entry["id"] not in {"steam", "epic-games-store", "gog-galaxy", "battle-net", "ea-app", "ubisoft-connect", "itch-io-app", "rockstar-games-launcher"}:
            raise CatalogError(f"catalog.entries: no automatic installer for {entry['id']!r}")
    ids = [entry["id"] for entry in entries]
    if len(ids) != len(set(ids)):
        raise CatalogError("catalog.entries: duplicate ID")
    names = [entry["name"].casefold() for entry in entries]
    if len(names) != len(set(names)):
        raise CatalogError("catalog.entries: duplicate name")
    return len(entries)


def main() -> int:
    path = Path(sys.argv[1]) if len(sys.argv) == 2 else DEFAULT_CATALOG_PATH
    if len(sys.argv) > 2:
        print("usage: python3 catalog/validate.py [catalog.json]", file=sys.stderr)
        return 2
    try:
        schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
        data = json.loads(path.read_text(encoding="utf-8"))
        count = validate_catalog(data, schema)
    except (OSError, json.JSONDecodeError, CatalogError, KeyError) as error:
        print(f"Invalid catalog: {error}", file=sys.stderr)
        return 1
    print(f"Valid catalog: {count} entries with untested compatibility")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
