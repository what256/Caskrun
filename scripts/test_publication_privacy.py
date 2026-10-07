#!/usr/bin/env python3
"""Integration checks for source-only publication and Git-history privacy."""
from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib

SCRIPT = Path(__file__).with_name("check-publication-privacy.py")
spec = importlib.util.spec_from_file_location("publication_guard", SCRIPT)
guard = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = guard
spec.loader.exec_module(guard)


def png(extra: bytes = b"") -> bytes:
    def chunk(tag: bytes, value: bytes) -> bytes:
        return struct.pack(">I", len(value)) + tag + value + struct.pack(">I", zlib.crc32(tag + value) & 0xFFFFFFFF)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 6, 0, 0, 0))
            + extra + chunk(b"IDAT", zlib.compress(b"\x00\xff\xff\xff\xff")) + chunk(b"IEND", b""))


def png_metadata() -> bytes:
    tag = b"tEXt"
    value = b"Author\x00synthetic owner"
    return struct.pack(">I", len(value)) + tag + value + struct.pack(">I", zlib.crc32(tag + value) & 0xFFFFFFFF)


class ContentTests(unittest.TestCase):
    def categories(self, path: str, content: bytes, mode: str = "100644", terms: list[str] | None = None) -> set[str]:
        scanner = guard.Scanner(terms or [])
        scanner.scan_blob(path, mode, content)
        return {finding.category for finding in scanner.findings}

    def test_clean_source_and_documented_reserved_urls(self):
        data = b"Demo phase. More features are coming. https://example.test/file\n"
        self.assertEqual(self.categories("README.md", data), set())
        fixture = "https://" + "user:password@" + "example.org/download"
        self.assertEqual(self.categories("catalog/test_validate.py", fixture.encode()), set())
        self.assertEqual(self.categories("core/src/sample.rs", b'let home = env::var("HOME");'), set())

    def test_concrete_local_paths(self):
        candidates = [
            "/" + "Users/sample-person/private.txt",
            "/" + "home/sample-person/private.txt",
            "/" + "private/" + "var/" + "folders/aa/sample-folder/T/session.txt",
            "/" + "tmp/local-session/secrets.txt",
            "C:" + "\\Users\\sample-person\\private.txt",
        ]
        for candidate in candidates:
            with self.subTest(kind=candidate.split("/")[0]):
                self.assertIn("concrete local path", self.categories("docs/note.md", candidate.encode()))

    def test_common_tokens_and_private_key(self):
        tokens = [
            "gh" + "p_" + "A" * 36,
            "github" + "_pat_" + "A" * 50,
            "sk" + "-proj-" + "A" * 40,
            "AK" + "IA" + "A" * 16,
            "AI" + "za" + "A" * 35,
            "xox" + "b-" + "1" * 30,
            "gl" + "pat-" + "A" * 30,
            "Bearer " + "A" * 40,
            "-----BEGIN " + "PRIVATE KEY-----",
        ]
        for token in tokens:
            self.assertTrue(self.categories("app/fixture.txt", token.encode()))

    def test_credential_assignment_and_url(self):
        assignment = 'api_' + 'key = "' + 'synthetic-long-credential' + '"'
        url = 'https://' + 'operator:synthetic-pass@' + 'public-host.com/download'
        self.assertIn("credential assignment", self.categories("config.py", assignment.encode()))
        self.assertIn("credentialled URL", self.categories("config.py", url.encode()))
        self.assertEqual(self.categories("config.py", b'password = "placeholder"'), set())

    def test_artifacts_and_symlinks(self):
        for path in ["dist/build.txt", ".build/output.txt", "core/target/trace.txt", "bottles/id/meta.json", "logs/run.txt", ".env", "source/archive.zip", "publication-private.json", "runtime/wine", "screenshots/window.png"]:
            self.assertTrue(self.categories(path, b"sample source"))
        self.assertIn("symlink, submodule or unsupported entry", self.categories("source/link", b"README.md", "120000"))
        self.assertIn("symlink, submodule or unsupported entry", self.categories("dependency", b"", "160000"))

    def test_only_clean_original_logos_are_permitted(self):
        self.assertEqual(self.categories("assets/caskrun-icon.png", png()), set())
        self.assertEqual(self.categories("assets/caskrun-menubar-template.png", png()), set())
        self.assertIn("unapproved image", self.categories("assets/other.png", png()))
        self.assertIn("logo metadata or unapproved chunk", self.categories("assets/caskrun-icon.png", png(png_metadata())))
        self.assertIn("invalid logo PNG", self.categories("assets/caskrun-icon.png", png() + b"trailing"))
        self.assertIn("unapproved binary", self.categories("source/unknown", b"\x00binary"))

    def test_private_terms_and_diagnostics_redact_paths(self):
        term = "hidden" + "owner"
        scanner = guard.Scanner([term])
        scanner.scan_blob("docs/" + term + ".md", "100644", ("line one\n" + term.upper()).encode())
        self.assertTrue(scanner.findings)
        self.assertTrue(all(term not in finding.path.lower() for finding in scanner.findings))
        self.assertIn(2, {finding.line for finding in scanner.findings})

    def test_private_identifier_boundaries_keep_file_and_path_protection(self):
        term = "private" + "owner"
        ordinary = "prefix" + term + "suffix"
        scanner = guard.Scanner([term])
        scanner.scan_blob("docs/" + ordinary + ".md", "100644", ordinary.encode())
        self.assertEqual(scanner.findings, set())
        self.assertEqual(scanner.safe_path("docs/" + ordinary + ".md"), "docs/" + ordinary + ".md")
        for basename in [term + ".md", "profile-" + term + ".md", "profile_" + term + ".md"]:
            matching = guard.Scanner([term])
            matching.scan_blob("docs/" + basename, "100644", ("identifier: " + term.upper()).encode())
            self.assertIn("configured private term", {finding.category for finding in matching.findings})
            self.assertTrue(all(term not in finding.path for finding in matching.findings))
        local_path = "/" + "Users/" + term + "/private.txt"
        self.assertIn("configured private term", self.categories("note.txt", local_path.encode(), terms=[term]))
        self.assertIn("concrete local path", self.categories("note.txt", local_path.encode(), terms=[term]))
        # Complex values retain literal matching even inside adjacent text.
        complex_term = "sample" + "@example.test"
        self.assertIn("configured private term", self.categories("note.txt", ("prefix" + complex_term + "suffix").encode(), terms=[complex_term]))

    def test_delete_only_push_input_is_safe(self):
        zero = "0" * 40
        self.assertEqual(guard.pre_push_revisions(f"(delete) {zero} refs/heads/obsolete {'a' * 40}\n"), [])
        with self.assertRaises(guard.GuardError):
            guard.pre_push_revisions("malformed input")


class RepositoryTests(unittest.TestCase):
    def setUp(self):
        work = SCRIPT.parent.parent / ".build" / "publication-privacy-tests"
        work.mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=work)
        self.repo = Path(self.temp.name)
        self.environment = os.environ.copy()
        for key in list(self.environment):
            if key.startswith("GIT_"):
                self.environment.pop(key)
        self.environment["TZ"] = "UTC"
        self.environment["GIT_AUTHOR_DATE"] = "2001-01-01T00:00:00+0000"
        self.environment["GIT_COMMITTER_DATE"] = "2001-01-01T00:00:00+0000"
        self.git("init", "-q")
        self.git("config", "user.name", guard.NEUTRAL_NAME)
        self.git("config", "user.email", guard.NEUTRAL_EMAIL)
        self.configure()
        (self.repo / "README.md").write_text("Caskrun demo. More features are coming.\n")

    def tearDown(self):
        self.temp.cleanup()

    def git(self, *args: str, env: dict | None = None) -> str:
        result = subprocess.run(["git", *args], cwd=self.repo, env=env or self.environment, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, "Synthetic repository Git operation failed.")
        return result.stdout.strip()

    def configure(self):
        (self.repo / ".git" / "publication-private.json").write_text(json.dumps({"terms": ["hidden" + "owner"]}))

    def check(self, *args: str, input_text: str | None = None, env: dict | None = None):
        return subprocess.run([sys.executable, str(SCRIPT), *args], cwd=self.repo, env=env or self.environment, input=input_text, capture_output=True, text=True)

    def commit(self):
        self.git("add", ".")
        self.git("commit", "-qm", "Add original demo source")
        return self.git("rev-parse", "HEAD")

    def test_clean_index_and_revision(self):
        for name in guard.LOGOS:
            p = self.repo / name
            p.parent.mkdir(exist_ok=True)
            p.write_bytes(png())
        self.git("add", ".")
        self.assertEqual(self.check("--staged").returncode, 0)
        self.commit()
        self.assertEqual(self.check("--revision", "HEAD").returncode, 0)

    def test_staged_reads_index_not_working_tree(self):
        private = "hidden" + "owner"
        (self.repo / "README.md").write_text(private)
        self.git("add", ".")
        (self.repo / "README.md").write_text("Clean working tree")
        result = self.check("--staged")
        self.assertEqual(result.returncode, 1)
        self.assertNotIn(private, result.stdout + result.stderr)
        self.assertIn("configured private term", result.stderr)

    def test_non_neutral_identity_and_environment_override(self):
        self.git("add", ".")
        self.git("config", "user.name", "Synthetic Individual")
        self.assertEqual(self.check("--staged").returncode, 1)
        self.git("config", "user.name", guard.NEUTRAL_NAME)
        env = dict(self.environment, GIT_AUTHOR_EMAIL="synthetic" + "@example.test")
        self.assertEqual(self.check("--staged", env=env).returncode, 1)

    def test_missing_local_configuration_fails_closed(self):
        self.git("add", ".")
        (self.repo / ".git" / "publication-private.json").unlink()
        self.assertEqual(self.check("--staged").returncode, 2)
        (self.repo / ".git" / "publication-private.json").write_text('{"terms": [1]}')
        self.assertEqual(self.check("--staged").returncode, 2)

    def test_old_history_still_blocks_after_file_is_clean(self):
        (self.repo / "README.md").write_text("hidden" + "owner")
        self.commit()
        (self.repo / "README.md").write_text("Clean current source")
        tip = self.commit()
        self.assertEqual(self.check("--revision", tip).returncode, 1)
        zero = "0" * 40
        pushed = self.check("--pre-push", input_text=f"refs/heads/main {tip} refs/heads/main {zero}\n")
        self.assertEqual(pushed.returncode, 1)
        self.assertNotIn("hidden" + "owner", pushed.stderr)

    def test_non_neutral_old_commit_and_utc(self):
        self.git("config", "user.name", "Synthetic Individual")
        self.commit()
        self.git("config", "user.name", guard.NEUTRAL_NAME)
        (self.repo / "README.md").write_text("Second clean change")
        self.commit()
        self.assertEqual(self.check("--revision", "HEAD").returncode, 1)

    def test_non_utc_metadata_and_private_message(self):
        self.git("add", ".")
        env = dict(self.environment, GIT_AUTHOR_DATE="2001-01-01T00:00:00+0200", GIT_COMMITTER_DATE="2001-01-01T00:00:00+0200")
        self.git("commit", "-qm", "Add demo", env=env)
        result = self.check("--revision", "HEAD")
        self.assertEqual(result.returncode, 1)
        self.assertIn("non-UTC commit metadata", result.stderr)
        (self.repo / "README.md").write_text("Changed")
        self.git("add", ".")
        self.git("commit", "-qm", "hidden" + "owner")
        result = self.check("--revision", "HEAD")
        self.assertIn("configured private term", result.stderr)
        self.assertNotIn("hidden" + "owner", result.stderr)

    def test_annotated_tag_identity_and_private_message_are_checked(self):
        tip = self.commit()
        self.git("tag", "-a", "demo-clean", "-m", "Demo release")
        self.assertEqual(self.check("--revision", "demo-clean").returncode, 0)
        self.git("tag", "-a", "demo-private", "-m", "hidden" + "owner")
        zero = "0" * 40
        tag_oid = self.git("rev-parse", "demo-private")
        result = self.check("--pre-push", input_text=f"refs/tags/demo-private {tag_oid} refs/tags/demo-private {zero}\n")
        self.assertEqual(result.returncode, 1)
        self.assertNotIn("hidden" + "owner", result.stderr)
        self.git("config", "user.name", "Synthetic Individual")
        self.git("tag", "-a", "demo-personal", "-m", "Demo release")
        result = self.check("--revision", "demo-personal")
        self.assertEqual(result.returncode, 1)
        self.assertIn("non-neutral commit identity", result.stderr)

    def test_scanner_and_tests_are_publishable_source(self):
        (self.repo / "scripts").mkdir()
        shutil.copyfile(SCRIPT, self.repo / "scripts" / SCRIPT.name)
        shutil.copyfile(Path(__file__), self.repo / "scripts" / Path(__file__).name)
        self.git("add", ".")
        result = self.check("--staged")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_pre_push_scans_each_tip_and_accepts_deletion(self):
        tip = self.commit()
        zero = "0" * 40
        result = self.check("--pre-push", input_text=f"refs/heads/main {tip} refs/heads/main {zero}\n(delete) {zero} refs/heads/old {tip}\n")
        self.assertEqual(result.returncode, 0)
        result = self.check("--pre-push", input_text=f"refs/heads/main not-an-object refs/heads/main {zero}\n")
        self.assertEqual(result.returncode, 2)


if __name__ == "__main__":
    unittest.main()
