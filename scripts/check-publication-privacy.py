#!/usr/bin/env python3
"""Fail closed before committing or pushing Caskrun source.

Local private terms live exclusively in a git directory's publication-private.json,
this worktree's or the one shared by every worktree of the repository. Findings
include a file, line and category, never the matched contents. Reachable history
is scanned, because deleting a sensitive file does not remove it from Git.
"""
from __future__ import annotations

import argparse
import dataclasses
import json
import os
from pathlib import Path, PurePosixPath
import re
import struct
import subprocess
import sys
import urllib.parse
import zlib

NEUTRAL_NAME = "Caskrun Contributors"
NEUTRAL_EMAIL = "contributors@caskrun.invalid"
LOGOS = {"assets/caskrun-icon.png", "assets/caskrun-menubar-template.png"}
PNG_CHUNKS = {b"IHDR", b"PLTE", b"tRNS", b"IDAT", b"IEND", b"sRGB", b"gAMA", b"cHRM", b"iCCP"}
FORBIDDEN_PARTS = {
    ".git", ".build", "dist", "target", "bottles", "logs", "screenshots",
    "secrets", "runtimes", "runtime", "node_modules", "__pycache__", "downloads",
}
FORBIDDEN_SUFFIXES = {
    ".app", ".dmg", ".cxarchive", ".zip", ".tar", ".tgz", ".gz", ".bz2",
    ".xz", ".7z", ".rar", ".exe", ".dll", ".dylib", ".so", ".a", ".o",
    ".icns", ".jpg", ".jpeg", ".gif", ".webp", ".heic", ".mp4", ".mov",
    ".log", ".jsonl", ".sqlite", ".sqlite3", ".db", ".pyc", ".p12", ".pfx",
    ".key", ".pem", ".mobileprovision", ".bundle",
}
PATH_PATTERNS = [
    re.compile(r"/(?:Users|home)/[A-Za-z0-9_.-]+(?:/|\b)"),
    re.compile(r"(?:/private)?/var/folders/[A-Za-z0-9_.-]+(?:/|\b)"),
    re.compile(r"(?:/private)?/tmp/[A-Za-z0-9_.-]+(?:/|\b)"),
    re.compile(r"[A-Za-z]:[\\/](?:Users|Documents and Settings)[\\/][A-Za-z0-9_.-]+", re.I),
]
SECRET_PATTERNS = [
    ("private key", re.compile(r"-----BEGIN (?:[A-Z0-9]+ )?PRIVATE KEY-----")),
    ("credential token", re.compile(r"\bgh[pousr]_[A-Za-z0-9]{30,}\b")),
    ("credential token", re.compile(r"\bgithub_pat_[A-Za-z0-9_]{30,}\b")),
    ("credential token", re.compile(r"\bsk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{24,}\b")),
    ("credential token", re.compile(r"\bAKIA[A-Z0-9]{16}\b")),
    ("credential token", re.compile(r"\bAIza[A-Za-z0-9_-]{30,}\b")),
    ("credential token", re.compile(r"\bxox[baprs]-[A-Za-z0-9-]{20,}\b")),
    ("credential token", re.compile(r"\bglpat-[A-Za-z0-9_-]{20,}\b")),
    ("credential token", re.compile(r"\b(?:npm_|pypi-)[A-Za-z0-9_-]{30,}\b")),
    ("credential token", re.compile(r"\b(?:sk|rk)_(?:live|test)_[A-Za-z0-9]{20,}\b")),
    ("authorization value", re.compile(r"\bBearer\s+[A-Za-z0-9_.=-]{20,}", re.I)),
    ("credential assignment", re.compile(
        r"\b(?:password|passwd|api[_-]?key|access[_-]?token|client[_-]?secret|secret[_-]?key)"
        r"\s*[:=]\s*[\"']([^\"'\r\n]{8,})[\"']", re.I)),
]
URL_PATTERN = re.compile(r"(?:https?|ssh|git|ftp)://[^\s\"'<>`]+", re.I)
PLACEHOLDERS = {"password", "your-password", "your_api_key", "your-api-key", "changeme", "placeholder", "example", "redacted"}


class GuardError(Exception):
    """A safe, non-sensitive diagnostic that can be printed directly."""


@dataclasses.dataclass(frozen=True)
class Finding:
    path: str
    line: int
    category: str


class Scanner:
    def __init__(self, terms: list[str]):
        self.terms = terms
        self.findings: set[Finding] = set()

    @staticmethod
    def term_pattern(term: str) -> re.Pattern[str]:
        literal = re.escape(term)
        if re.fullmatch(r"[A-Za-z0-9]+", term):
            # Match whole identifiers and filename/path components. Underscores
            # remain separators; ordinary longer words are not private matches.
            literal = r"(?<![A-Za-z0-9])" + literal + r"(?![A-Za-z0-9])"
        return re.compile(literal, re.I)

    def safe_path(self, path: str) -> str:
        result = path
        for term in sorted(self.terms, key=len, reverse=True):
            result = self.term_pattern(term).sub("[private]", result)
        # Escape controls so a path cannot forge diagnostics.
        return result.encode("unicode_escape").decode("ascii")

    def add(self, path: str, line: int, category: str) -> None:
        self.findings.add(Finding(self.safe_path(path), line, category))

    def scan_text(self, path: str, text: str, *, path_only: bool = False) -> None:
        def finding(offset: int, category: str) -> None:
            self.add(path, 0 if path_only else text.count("\n", 0, offset) + 1, category)

        for term in self.terms:
            for match in self.term_pattern(term).finditer(text):
                finding(match.start(), "configured private term")
        for pattern in PATH_PATTERNS:
            for match in pattern.finditer(text):
                finding(match.start(), "concrete local path")
        for category, pattern in SECRET_PATTERNS:
            for match in pattern.finditer(text):
                if category == "credential assignment" and match.group(1).lower() in PLACEHOLDERS:
                    continue
                finding(match.start(), category)
        for match in URL_PATTERN.finditer(text):
            try:
                parsed = urllib.parse.urlsplit(match.group().rstrip(",);]}"))
                host = parsed.hostname
            except ValueError:
                finding(match.start(), "invalid credential URL")
                continue
            if host and "@" in parsed.netloc and not reserved_host(host):
                finding(match.start(), "credentialled URL")

    def scan_blob(self, path: str, mode: str, data: bytes) -> None:
        self.scan_text(path, path, path_only=True)
        parts = PurePosixPath(path).parts
        if any(part.lower() in FORBIDDEN_PARTS or part.lower().endswith(".app") for part in parts):
            self.add(path, 0, "excluded artifact path")
        basename = PurePosixPath(path).name.lower()
        suffix = PurePosixPath(path).suffix.lower()
        if basename.startswith(".env") or basename == "publication-private.json" or suffix in FORBIDDEN_SUFFIXES:
            self.add(path, 0, "excluded artifact file")
        if mode not in {"100644", "100755"}:
            self.add(path, 0, "symlink, submodule or unsupported entry")
            return
        if path in LOGOS:
            self.scan_png(path, data)
            return
        if suffix == ".png":
            self.add(path, 0, "unapproved image")
        if b"\x00" in data:
            self.add(path, 0, "unapproved binary")
            return
        try:
            content = data.decode("utf-8")
        except UnicodeDecodeError:
            self.add(path, 0, "unapproved binary")
            return
        self.scan_text(path, content)

    def scan_png(self, path: str, data: bytes) -> None:
        if not data.startswith(b"\x89PNG\r\n\x1a\n"):
            self.add(path, 0, "invalid logo PNG")
            return
        offset = 8
        saw_header = saw_data = saw_end = False
        while offset + 12 <= len(data):
            size = struct.unpack(">I", data[offset:offset + 4])[0]
            tag = data[offset + 4:offset + 8]
            end = offset + 12 + size
            if end > len(data) or size > 32 * 1024 * 1024:
                self.add(path, 0, "invalid logo PNG")
                return
            payload = data[offset + 8:offset + 8 + size]
            crc = struct.unpack(">I", data[offset + 8 + size:end])[0]
            if zlib.crc32(tag + payload) & 0xFFFFFFFF != crc:
                self.add(path, 0, "invalid logo PNG checksum")
            if tag not in PNG_CHUNKS:
                self.add(path, 0, "logo metadata or unapproved chunk")
            # Private terms cannot be hidden in ancillary payloads or raw bytes.
            if tag != b"IDAT":
                self.scan_text(path, payload.decode("latin1"))
            if tag == b"IHDR":
                saw_header = offset == 8 and size == 13
            elif tag == b"IDAT":
                saw_data = True
            elif tag == b"iCCP":
                # ICC names and decompressed profile metadata are also checked.
                try:
                    separator = payload.index(b"\x00")
                    if payload[separator + 1] != 0:
                        raise ValueError
                    profile = zlib.decompress(payload[separator + 2:])
                    self.scan_text(path, profile.decode("latin1"))
                except (ValueError, IndexError, zlib.error):
                    self.add(path, 0, "invalid logo color profile")
            elif tag == b"IEND":
                saw_end = size == 0 and end == len(data)
                offset = end
                break
            offset = end
        if not (saw_header and saw_data and saw_end) or offset != len(data):
            self.add(path, 0, "invalid logo PNG")
        # Search terms in the entire file, including bytes outside text chunks.
        raw = data.decode("latin1")
        for term in self.terms:
            if self.term_pattern(term).search(raw):
                self.add(path, 0, "configured private term in logo")

    def scan_identity(self, label: str, signature: str, *, require_utc: bool) -> None:
        match = re.fullmatch(r"(.*?) <([^<>]*)> ([0-9]+) ([+-][0-9]{4})", signature)
        if not match or match.group(1) != NEUTRAL_NAME or match.group(2) != NEUTRAL_EMAIL:
            self.add(label, 0, "non-neutral commit identity")
        elif require_utc and match.group(4) != "+0000":
            self.add(label, 0, "non-UTC commit metadata")
        self.scan_text(label, signature, path_only=True)


def reserved_host(host: str) -> bool:
    host = host.lower().rstrip(".")
    return (host in {"example.com", "example.org", "example.net", "localhost"}
            or host.endswith((".example", ".test", ".invalid")))


def git(*args: str, input_data: bytes | None = None) -> bytes:
    result = subprocess.run(["git", *args], input=input_data, capture_output=True)
    if result.returncode:
        raise GuardError("Git inspection failed; no source was approved.")
    return result.stdout


def local_terms() -> list[str]:
    # A linked worktree has its own git directory, so also accept the shared one
    # every worktree of this repository uses. Both must be regular files, and an
    # absent configuration still fails closed.
    candidates = [Path(os.fsdecode(git("rev-parse", "--git-path", "publication-private.json").strip()))]
    common = Path(os.fsdecode(git("rev-parse", "--git-common-dir").strip())) / "publication-private.json"
    if common not in candidates:
        candidates.append(common)
    config = None
    for path in candidates:
        if path.is_symlink():
            raise GuardError("The local privacy configuration must be a regular file.")
        try:
            config = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            continue
        break
    if config is None:
        raise GuardError("Missing or invalid git-local publication-private.json; configure local private terms first.")
    if not isinstance(config, dict) or not isinstance(config.get("terms"), list):
        raise GuardError("The local privacy configuration must contain a terms list.")
    terms = config["terms"]
    if any(not isinstance(term, str) or len(term.strip()) < 3 or "\x00" in term for term in terms):
        raise GuardError("Private terms must be strings with at least three non-space characters.")
    return list(dict.fromkeys(terms))


def scan_staged(scanner: Scanner) -> None:
    # Inspect the complete index, not only changed lines, and do not trust the
    # working tree version of a file that may differ from the staged version.
    for identity in ("GIT_AUTHOR_IDENT", "GIT_COMMITTER_IDENT"):
        scanner.scan_identity("<staged " + identity.lower() + ">", os.fsdecode(git("var", identity)).strip(), require_utc=False)
    for record in git("ls-files", "--stage", "-z").split(b"\x00"):
        if not record:
            continue
        metadata, path = record.split(b"\t", 1)
        mode, oid, stage = metadata.decode("ascii").split()
        name = os.fsdecode(path)
        if stage != "0":
            scanner.add(name, 0, "unmerged index entry")
            continue
        if mode == "160000":
            scanner.scan_blob(name, mode, b"")
        else:
            scanner.scan_blob(name, mode, git("cat-file", "blob", oid))


def resolve_revision(revision: str) -> str:
    oid = git("rev-parse", "--verify", "--end-of-options", revision + "^{commit}").decode("ascii").strip()
    if not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", oid):
        raise GuardError("The requested revision is not a commit.")
    return oid


def scan_tag_chain(scanner: Scanner, revision: str) -> None:
    oid = git("rev-parse", "--verify", "--end-of-options", revision).decode("ascii").strip()
    seen: set[str] = set()
    while git("cat-file", "-t", oid).strip() == b"tag":
        if oid in seen:
            raise GuardError("Invalid tag chain; push blocked.")
        seen.add(oid)
        raw = git("cat-file", "tag", oid)
        headers, separator, message = raw.partition(b"\n\n")
        label = "<tag " + oid[:12] + ">"
        identity = target = None
        for line in headers.splitlines():
            if line.startswith(b"tagger "):
                identity = os.fsdecode(line[7:])
            elif line.startswith(b"object "):
                target = line[7:].decode("ascii", errors="replace")
        if not separator or identity is None or not target or not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", target):
            scanner.add(label, 0, "invalid annotated tag metadata")
            return
        scanner.scan_identity(label + " tagger", identity, require_utc=True)
        scanner.scan_text(label + " metadata", headers.decode("utf-8", errors="replace"))
        scanner.scan_text(label + " message", message.decode("utf-8", errors="replace"))
        oid = target


def scan_revisions(scanner: Scanner, revisions: list[str]) -> tuple[int, int]:
    for revision in revisions:
        scan_tag_chain(scanner, revision)
    tips = [resolve_revision(revision) for revision in revisions]
    if not tips:
        return 0, 0
    commits = git("rev-list", "--topo-order", *tips).decode("ascii").splitlines()
    seen_blobs: set[tuple[str, str, str]] = set()
    for oid in commits:
        raw = git("cat-file", "commit", oid)
        headers, separator, message = raw.partition(b"\n\n")
        label = "<commit " + oid[:12] + ">"
        identities: set[str] = set()
        for line in headers.splitlines():
            for prefix in (b"author ", b"committer "):
                if line.startswith(prefix):
                    identities.add(prefix.decode().strip())
                    scanner.scan_identity(label + " " + prefix.decode().strip(), os.fsdecode(line[len(prefix):]), require_utc=True)
        if not separator or identities != {"author", "committer"}:
            scanner.add(label, 0, "invalid commit metadata")
        scanner.scan_text(label + " metadata", headers.decode("utf-8", errors="replace"))
        scanner.scan_text(label + " message", message.decode("utf-8", errors="replace"))
        for record in git("ls-tree", "-r", "-z", oid).split(b"\x00"):
            if not record:
                continue
            metadata, path = record.split(b"\t", 1)
            mode, kind, blob_oid = metadata.decode("ascii").split()
            name = os.fsdecode(path)
            key = (name, mode, blob_oid)
            if key in seen_blobs:
                continue
            seen_blobs.add(key)
            scanner.scan_blob(name, mode, git("cat-file", "blob", blob_oid) if kind == "blob" else b"")
    return len(commits), len(seen_blobs)


def pre_push_revisions(stream: str) -> list[str]:
    revisions = []
    for line in stream.splitlines():
        fields = line.split()
        if len(fields) != 4:
            raise GuardError("Invalid pre-push input; push blocked.")
        _local_ref, local_oid, _remote_ref, remote_oid = fields
        if not re.fullmatch(r"(?:[0-9a-f]{40}|[0-9a-f]{64})", local_oid) or not re.fullmatch(r"(?:[0-9a-f]{40}|[0-9a-f]{64})", remote_oid):
            raise GuardError("Invalid pre-push object identifier; push blocked.")
        if local_oid.strip("0"):
            revisions.append(local_oid)
    return list(dict.fromkeys(revisions))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--staged", action="store_true")
    mode.add_argument("--revision", metavar="COMMIT")
    mode.add_argument("--pre-push", action="store_true")
    args = parser.parse_args()
    try:
        scanner = Scanner(local_terms())
        if args.staged:
            scan_staged(scanner)
            summary = "complete staged index and effective commit identities"
        else:
            revisions = [args.revision] if args.revision else pre_push_revisions(sys.stdin.read())
            count, blobs = scan_revisions(scanner, revisions)
            summary = f"{count} reachable commits and {blobs} source entries"
        if scanner.findings:
            print("Publication privacy check BLOCKED:", file=sys.stderr)
            for item in sorted(scanner.findings, key=lambda f: (f.path, f.line, f.category)):
                print(f"  {item.path}:{item.line}: {item.category}", file=sys.stderr)
            print("No matched values are shown. Review locally before committing or pushing.", file=sys.stderr)
            return 1
        print(f"Publication privacy check passed: {summary}.")
        return 0
    except GuardError as exc:
        print("Publication privacy check BLOCKED: " + str(exc), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
