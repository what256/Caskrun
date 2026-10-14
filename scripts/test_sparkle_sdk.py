"""Pinned SDK/cache validation checks; no network, Wine, or app launches."""
import hashlib
import importlib.util
import io
import os
from pathlib import Path
import shutil
import tarfile
import tempfile
import unittest
from unittest import mock

SCRIPT = Path(__file__).with_name("prepare-sparkle.py")
SPEC = importlib.util.spec_from_file_location("sparkle_sdk", SCRIPT)
SDK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SDK)


def archive_bytes(entries):
    result = io.BytesIO()
    with tarfile.open(fileobj=result, mode="w:xz") as archive:
        for name, kind, payload in entries:
            member = tarfile.TarInfo(name)
            member.mode = 0o755 if name.endswith("/Sparkle") else 0o644
            if kind == "file":
                member.size = len(payload)
                archive.addfile(member, io.BytesIO(payload))
            else:
                member.type = {"link": tarfile.SYMTYPE, "hardlink": tarfile.LNKTYPE,
                               "fifo": tarfile.FIFOTYPE, "directory": tarfile.DIRTYPE}[kind]
                if kind in ("link", "hardlink"):
                    member.linkname = payload
                archive.addfile(member)
    return result.getvalue()


FIXTURE_ENTRIES = [
    ("./Sparkle.framework/Versions/B/Sparkle", "file", b"independent fake framework executable"),
    ("./Sparkle.framework/Versions/B/Resources/Info.plist", "file", b"independent test info"),
    ("./Sparkle.framework/Versions/Current", "link", "B"),
    ("./Sparkle.framework/Sparkle", "link", "Versions/Current/Sparkle"),
    ("./Sparkle.framework/Resources", "link", "Versions/Current/Resources"),
    ("./LICENSE", "file", b"independent fixture license"),
]


class SparkleSDKTests(unittest.TestCase):
    def setUp(self):
        build = SCRIPT.parent.parent / ".build"
        build.mkdir(exist_ok=True)
        self.root = Path(tempfile.mkdtemp(prefix="sparkle-sdk-tests-", dir=build))
        self.cache = self.root / "cache"
        self.cache.mkdir()
        self.sdk = self.cache / ("Sparkle-" + SDK.VERSION)
        self.archive = self.cache / ("Sparkle-" + SDK.VERSION + ".tar.xz")
        self.data = archive_bytes(FIXTURE_ENTRIES)
        self.digest = hashlib.sha256(self.data).hexdigest()
        self.archive.write_bytes(self.data)

    def tearDown(self):
        shutil.rmtree(self.root)

    def prepare(self, check=False):
        return SDK.prepare(self.cache, check=check, expected_digest=self.digest)

    def test_pinned_upstream_digest_is_not_a_cli_override(self):
        self.assertEqual(SDK.VERSION, "2.10.0")
        self.assertEqual(SDK.ARCHIVE_SHA256, "c2bf58aa8387266ac179357b1415d6f2635f044da8be41042af32425dae6da0c")
        self.assertIn("sparkle-project/Sparkle/releases/download/2.10.0", SDK.ARCHIVE_URL)
        with self.assertRaisesRegex(SDK.SDKError, "checksum"):
            SDK.verified_bytes(self.archive)

    def test_safe_extraction_preserves_framework_links_and_executable(self):
        framework = self.prepare()
        self.assertEqual(framework, self.sdk / "Sparkle.framework")
        self.assertEqual(os.readlink(framework / "Sparkle"), "Versions/Current/Sparkle")
        self.assertEqual(os.readlink(framework / "Versions/Current"), "B")
        self.assertEqual((framework / "Sparkle").read_bytes(), FIXTURE_ENTRIES[0][2])
        self.assertEqual((framework / "Sparkle").stat().st_mode & 0o777, 0o755)

    def test_cache_reuse_is_offline_and_does_not_rewrite_files(self):
        self.prepare()
        executable = self.sdk / "Sparkle.framework/Versions/B/Sparkle"
        before = executable.stat()
        with mock.patch.object(SDK, "download_archive", side_effect=AssertionError("must not download")):
            self.prepare(check=True)
            self.prepare()
        self.assertEqual((executable.stat().st_ino, executable.stat().st_mtime_ns), (before.st_ino, before.st_mtime_ns))
        self.assertEqual(sorted(path.name for path in self.cache.iterdir()), [self.sdk.name, self.archive.name])

    def test_check_never_creates_extraction(self):
        with self.assertRaisesRegex(SDK.SDKError, "never writes"):
            self.prepare(check=True)
        self.assertFalse(self.sdk.exists())

    def test_check_never_fetches_missing_archive(self):
        self.archive.unlink()
        with mock.patch.object(SDK, "download_archive", side_effect=AssertionError("must not download")):
            with self.assertRaisesRegex(SDK.SDKError, "never downloads"):
                self.prepare(check=True)

    def test_check_never_creates_missing_cache(self):
        missing = self.root / "missing"
        with self.assertRaises(SDK.SDKError):
            SDK.prepare(missing, check=True)
        self.assertFalse(missing.exists())

    def test_archive_corruption_refuses_before_extraction(self):
        self.archive.write_bytes(self.data + b"changed")
        with self.assertRaisesRegex(SDK.SDKError, "checksum"):
            self.prepare()
        self.assertFalse(self.sdk.exists())

    def test_archive_symlink_is_refused(self):
        real = self.root / "real.tar.xz"
        self.archive.rename(real)
        self.archive.symlink_to(real)
        with self.assertRaises(SDK.SDKError):
            self.prepare()
        self.assertFalse(self.sdk.exists())

    def test_archive_read_is_bounded(self):
        with mock.patch.object(SDK, "MAX_ARCHIVE_BYTES", len(self.data) - 1):
            with self.assertRaises(SDK.SDKError):
                self.prepare()
        self.assertFalse(self.sdk.exists())

    def test_cached_file_same_size_corruption_is_refused(self):
        self.prepare()
        file = self.sdk / "LICENSE"
        content = file.read_bytes()
        file.write_bytes(b"X" + content[1:])
        with self.assertRaisesRegex(SDK.SDKError, "content"):
            self.prepare()
        self.assertEqual(file.read_bytes(), b"X" + content[1:])

    def test_cached_file_permissions_are_verified(self):
        self.prepare()
        (self.sdk / "LICENSE").chmod(0o755)
        with self.assertRaisesRegex(SDK.SDKError, "permissions"):
            self.prepare(check=True)

    def test_cached_link_must_match_exact_archive_target(self):
        self.prepare()
        link = self.sdk / "Sparkle.framework/Sparkle"
        link.unlink()
        link.symlink_to("Versions/B/Sparkle")
        with self.assertRaisesRegex(SDK.SDKError, "symbolic link"):
            self.prepare(check=True)

    def test_cached_regular_file_cannot_be_replaced_with_link(self):
        self.prepare()
        license = self.sdk / "LICENSE"
        outside = self.root / "outside"
        outside.write_bytes(license.read_bytes())
        license.unlink()
        license.symlink_to(outside)
        with self.assertRaisesRegex(SDK.SDKError, "regular file"):
            self.prepare(check=True)
        self.assertEqual(outside.read_bytes(), FIXTURE_ENTRIES[-1][2])

    def test_cached_directory_cannot_be_replaced_with_link(self):
        self.prepare()
        versions = self.sdk / "Sparkle.framework/Versions"
        outside = self.root / "outside"
        versions.rename(outside)
        versions.symlink_to(outside)
        with self.assertRaisesRegex(SDK.SDKError, "directory was replaced"):
            self.prepare(check=True)

    def test_missing_and_extra_cached_files_are_refused(self):
        self.prepare()
        (self.sdk / "unexpected").write_bytes(b"new")
        with self.assertRaisesRegex(SDK.SDKError, "unexpected"):
            self.prepare(check=True)
        (self.sdk / "unexpected").unlink()
        (self.sdk / "LICENSE").unlink()
        with self.assertRaisesRegex(SDK.SDKError, "incomplete"):
            self.prepare(check=True)

    def test_sdk_root_cannot_be_substituted_with_link(self):
        self.prepare()
        actual = self.root / "actual"
        self.sdk.rename(actual)
        self.sdk.symlink_to(actual)
        with self.assertRaisesRegex(SDK.SDKError, "independent directory"):
            self.prepare(check=True)

    def test_cache_root_cannot_be_a_link(self):
        alias = self.root / "alias"
        alias.symlink_to(self.cache)
        with self.assertRaisesRegex(SDK.SDKError, "independent directory"):
            SDK.prepare(alias, expected_digest=self.digest)

    def test_traversal_absolute_and_windows_paths_are_refused(self):
        for name in ("../escape", "/absolute", "C:/escape", "a/../../escape", "a\\escape", "a\nname"):
            with self.subTest(name=name), self.assertRaises(SDK.SDKError):
                SDK.archive_manifest(archive_bytes([(name, "file", b"x")]))

    def test_escaping_absolute_broken_cyclic_and_ancestor_links_are_refused(self):
        for target in ("../../escape", "/absolute", "missing", "../", "../Sparkle.framework/link"):
            entries = FIXTURE_ENTRIES + [("Sparkle.framework/link", "link", target)]
            with self.subTest(target=target), self.assertRaises(SDK.SDKError):
                SDK.archive_manifest(archive_bytes(entries))
        cycle = FIXTURE_ENTRIES + [("Sparkle.framework/one", "link", "two"), ("Sparkle.framework/two", "link", "one")]
        with self.assertRaises(SDK.SDKError):
            SDK.archive_manifest(archive_bytes(cycle))

    def test_writes_through_archive_links_are_refused(self):
        entries = FIXTURE_ENTRIES + [("alias", "link", "Sparkle.framework/Versions/B"),
                                     ("alias/injected", "file", b"x")]
        with self.assertRaisesRegex(SDK.SDKError, "non-directory parent"):
            SDK.archive_manifest(archive_bytes(entries))

    def test_special_files_and_hardlinks_are_refused(self):
        for kind in ("fifo", "hardlink"):
            with self.subTest(kind=kind), self.assertRaises(SDK.SDKError):
                SDK.archive_manifest(archive_bytes(FIXTURE_ENTRIES + [("extra", kind, "LICENSE")]))

    def test_duplicate_paths_are_refused(self):
        with self.assertRaisesRegex(SDK.SDKError, "duplicate"):
            SDK.archive_manifest(archive_bytes(FIXTURE_ENTRIES + [("LICENSE", "file", b"other")]))

    def test_expanded_size_and_entry_count_are_bounded(self):
        with mock.patch.object(SDK, "MAX_EXPANDED_BYTES", 1):
            with self.assertRaisesRegex(SDK.SDKError, "expands"):
                SDK.archive_manifest(self.data)
        with mock.patch.object(SDK, "MAX_ENTRIES", 1):
            with self.assertRaisesRegex(SDK.SDKError, "too many"):
                SDK.archive_manifest(self.data)

    def test_download_overflow_is_removed_without_publishing_archive(self):
        self.archive.unlink()
        with mock.patch.object(SDK, "MAX_ARCHIVE_BYTES", 2), mock.patch.object(SDK.urllib.request, "urlopen", return_value=io.BytesIO(b"overflow")):
            with self.assertRaisesRegex(SDK.SDKError, "size limit"):
                SDK.download_archive(self.archive)
        self.assertEqual(list(self.cache.iterdir()), [])

    def test_wrong_download_checksum_is_removed(self):
        self.archive.unlink()
        with mock.patch.object(SDK.urllib.request, "urlopen", return_value=io.BytesIO(b"wrong digest")):
            with self.assertRaisesRegex(SDK.SDKError, "checksum"):
                SDK.download_archive(self.archive)
        self.assertEqual(list(self.cache.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
