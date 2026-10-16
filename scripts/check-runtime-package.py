#!/usr/bin/env python3
"""Require an exact engine approved by Core before packaging it.

Core's REVIEWED_RUNTIME_IDENTITIES array is the sole approval policy. This
reader intentionally accepts only its current explicit constant-list syntax;
an unfamiliar or ambiguous policy fails closed. No CLI identity override is
provided. Hidden files are hashed exactly like every other package entry.
"""
from __future__ import annotations

import argparse
import importlib.util
from pathlib import Path
import re
import sys


REPO = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location(
    "runtime_package_content", REPO / "scripts/package-wine-runtime-dxmt-dev.py"
)
PACKAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACKAGE)


class RuntimePackageError(Exception):
    pass


def rust_tokens(source: str) -> list[tuple[str, str]]:
    """Tokenize comments/strings so policy-looking text inside them is inert."""
    tokens = []
    cursor = 0
    while cursor < len(source):
        if source[cursor].isspace():
            cursor += 1
            continue
        if source.startswith("//", cursor):
            end = source.find("\n", cursor + 2)
            cursor = len(source) if end < 0 else end + 1
            continue
        if source.startswith("/*", cursor):
            cursor += 2
            depth = 1
            while cursor < len(source) and depth:
                if source.startswith("/*", cursor):
                    depth += 1
                    cursor += 2
                elif source.startswith("*/", cursor):
                    depth -= 1
                    cursor += 2
                else:
                    cursor += 1
            if depth:
                raise RuntimePackageError("Unterminated comment in Core runtime policy")
            continue
        raw = re.match(r'(?:br|r)(#*)"', source[cursor:])
        if raw:
            closing = '"' + raw.group(1)
            start = cursor + raw.end()
            end = source.find(closing, start)
            if end < 0:
                raise RuntimePackageError("Unterminated raw string in Core runtime policy")
            tokens.append(("raw", source[start:end]))
            cursor = end + len(closing)
            continue
        if source[cursor] == '"':
            start = cursor + 1
            cursor = start
            while cursor < len(source) and source[cursor] != '"':
                cursor += 2 if source[cursor] == "\\" else 1
            if cursor >= len(source):
                raise RuntimePackageError("Unterminated string in Core runtime policy")
            tokens.append(("string", source[start:cursor]))
            cursor += 1
            continue
        character = re.match(r"'(?:\\.|[^'\\\r\n])'", source[cursor:])
        if character:
            tokens.append(("character", character.group()))
            cursor += character.end()
            continue
        identifier = re.match(r"[A-Za-z_][A-Za-z0-9_]*", source[cursor:])
        if identifier:
            tokens.append(("identifier", identifier.group()))
            cursor += identifier.end()
            continue
        number = re.match(r"[0-9]+", source[cursor:])
        if number:
            tokens.append(("number", number.group()))
            cursor += number.end()
            continue
        tokens.append(("punctuation", source[cursor]))
        cursor += 1
    return tokens


def reviewed_identities(source: str) -> frozenset[str]:
    tokens = rust_tokens(source)
    declarations = [i for i in range(len(tokens) - 1)
                    if tokens[i:i + 2] == [("identifier", "const"),
                                          ("identifier", "REVIEWED_RUNTIME_IDENTITIES")]]
    if len(declarations) != 1:
        raise RuntimePackageError("Core must declare exactly one reviewed runtime array")
    start = declarations[0] + 2
    expected = [("punctuation", ":"), ("punctuation", "["),
                ("punctuation", "&"), ("identifier", "str"),
                ("punctuation", ";")]
    if tokens[start:start + 5] != expected or start + 5 >= len(tokens):
        raise RuntimePackageError("Unsupported reviewed runtime array declaration")
    count_token = tokens[start + 5]
    if count_token[0] != "number" or not 1 <= int(count_token[1]) <= 1000:
        raise RuntimePackageError("Invalid reviewed runtime array length")
    if tokens[start + 6:start + 9] != [("punctuation", "]"),
                                     ("punctuation", "="),
                                     ("punctuation", "[")]:
        raise RuntimePackageError("Unsupported reviewed runtime array initializer")
    cursor = start + 9
    names = []
    while cursor < len(tokens) and tokens[cursor] != ("punctuation", "]"):
        if tokens[cursor][0] != "identifier":
            raise RuntimePackageError("Reviewed runtime members must be explicit constants")
        names.append(tokens[cursor][1])
        cursor += 1
        if cursor < len(tokens) and tokens[cursor] == ("punctuation", ","):
            cursor += 1
        elif cursor >= len(tokens) or tokens[cursor] != ("punctuation", "]"):
            raise RuntimePackageError("Invalid reviewed runtime member separator")
    if tokens[cursor:cursor + 2] != [("punctuation", "]"), ("punctuation", ";")]:
        raise RuntimePackageError("Incomplete reviewed runtime array")
    if len(names) != int(count_token[1]) or len(set(names)) != len(names):
        raise RuntimePackageError("Reviewed runtime array count or members are inconsistent")
    approved = []
    for name in names:
        definitions = [i for i in range(len(tokens) - 1)
                       if tokens[i:i + 2] == [("identifier", "const"), ("identifier", name)]]
        if len(definitions) != 1:
            raise RuntimePackageError(f"Runtime constant {name} is missing or ambiguous")
        value = definitions[0] + 2
        if tokens[value:value + 4] != [("punctuation", ":"), ("punctuation", "&"),
                                       ("identifier", "str"), ("punctuation", "=")]:
            raise RuntimePackageError(f"Unsupported runtime constant {name}")
        literal = tokens[value + 4:value + 6]
        if (len(literal) != 2 or literal[0][0] != "string"
                or not re.fullmatch(r"sha256:[0-9a-f]{64}", literal[0][1])
                or literal[1] != ("punctuation", ";")):
            raise RuntimePackageError(f"Runtime constant {name} must be one exact SHA-256 literal")
        approved.append(literal[0][1])
    if len(set(approved)) != len(approved):
        raise RuntimePackageError("Core reviewed runtime array contains duplicate identities")
    return frozenset(approved)


def verify_package(runtime: Path, policy_source: str) -> str:
    approved = reviewed_identities(policy_source)
    # The shared tree inspector compares resolved internal symlinks against
    # this root. Make relative CLI paths absolute without following root links.
    runtime = runtime.absolute()
    try:
        identity = "sha256:" + PACKAGE.tree_identity(runtime)
    except (OSError, PACKAGE.PackageError) as error:
        raise RuntimePackageError(f"Could not verify complete runtime package: {error}") from error
    if identity not in approved:
        raise RuntimePackageError(
            "Refusing an unreviewed or changed Windows engine package: " + identity
            + ". Restore the exact reviewed package; extra Finder files also change its identity."
        )
    return identity


def verify_copy(runtime: Path, copy: Path, policy_source: str) -> str:
    original = verify_package(runtime, policy_source)
    copied = verify_package(copy, policy_source)
    if original != copied:
        raise RuntimePackageError("Packaged Windows engine differs from its exact reviewed source")
    return original


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("runtime", type=Path)
    parser.add_argument("copy", type=Path, nargs="?")
    args = parser.parse_args()
    try:
        policy = (REPO / "core/src/runtime_migration.rs").read_text(encoding="utf-8")
        identity = (verify_copy(args.runtime, args.copy, policy) if args.copy is not None
                    else verify_package(args.runtime, policy))
    except (OSError, UnicodeError, RuntimePackageError) as error:
        print(f"Runtime package check failed: {error}", file=sys.stderr)
        return 1
    print(f"Reviewed runtime package verified: {identity}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
