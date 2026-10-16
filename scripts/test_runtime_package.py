"""Reviewed-engine packaging gates, using disposable files only; never Wine."""
import importlib.util
import os
from pathlib import Path
import shutil
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location(
    "runtime_package_check", Path(__file__).with_name("check-runtime-package.py")
)
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


def policy(identities):
    declarations = "\n".join(f'const ENGINE_{i}: &str = "{value}";'
                             for i, value in enumerate(identities))
    members = ", ".join(f"ENGINE_{i}" for i in range(len(identities)))
    return (declarations + "\n"
            + f"pub(super) const REVIEWED_RUNTIME_IDENTITIES: [&str; {len(identities)}] = [{members}];")


class RuntimePackageGuards(unittest.TestCase):
    def setUp(self):
        build = CHECK.REPO / ".build"
        build.mkdir(exist_ok=True)
        self.root = Path(tempfile.mkdtemp(prefix="runtime-package-tests-", dir=build))
        self.runtime = self.root / "runtime"
        self.runtime.mkdir()
        (self.runtime / "engine").write_bytes(b"reviewed synthetic engine")
        (self.runtime / "engine").chmod(0o755)
        self.identity = "sha256:" + CHECK.PACKAGE.tree_identity(self.runtime)
        self.source = policy([self.identity])

    def tearDown(self):
        shutil.rmtree(self.root)

    def test_current_core_policy_parses_from_its_array(self):
        source = (CHECK.REPO / "core/src/runtime_migration.rs").read_text()
        values = CHECK.reviewed_identities(source)
        self.assertEqual(len(values), 12)
        self.assertIn("sha256:cae84f3978b2e0820bbdfc95c1e9dd5fa6c79c8d1f1aca825ac86bd2afb77c29", values)

    def test_reviewed_exact_copy_succeeds_after_relocation(self):
        copied = self.root / "copy"
        shutil.copytree(self.runtime, copied, symlinks=True)
        self.assertEqual(CHECK.verify_copy(self.runtime, copied, self.source), self.identity)

    def test_unknown_package_is_refused(self):
        with self.assertRaisesRegex(CHECK.RuntimePackageError, "unreviewed or changed"):
            CHECK.verify_package(self.runtime, policy(["sha256:" + "0" * 64]))

    def test_finder_residue_is_not_ignored_or_added_to_approval(self):
        (self.runtime / ".DS_Store").write_bytes(b"synthetic Finder metadata")
        with self.assertRaisesRegex(CHECK.RuntimePackageError, "unreviewed or changed"):
            CHECK.verify_package(self.runtime, self.source)

    def test_content_and_mode_changes_are_refused(self):
        engine = self.runtime / "engine"
        original = engine.read_bytes()
        engine.write_bytes(b"unreviewed synthetic engine")
        with self.assertRaises(CHECK.RuntimePackageError):
            CHECK.verify_package(self.runtime, self.source)
        engine.write_bytes(original)
        engine.chmod(0o644)
        with self.assertRaises(CHECK.RuntimePackageError):
            CHECK.verify_package(self.runtime, self.source)

    def test_two_individually_reviewed_but_different_packages_do_not_match(self):
        copied = self.root / "copy"
        shutil.copytree(self.runtime, copied)
        (copied / "engine").write_bytes(b"different reviewed synthetic engine")
        other = "sha256:" + CHECK.PACKAGE.tree_identity(copied)
        with self.assertRaisesRegex(CHECK.RuntimePackageError, "differs from its exact reviewed source"):
            CHECK.verify_copy(self.runtime, copied, policy([self.identity, other]))

    def test_stray_constants_are_not_approvals(self):
        source = policy(["sha256:" + "0" * 64])
        source += f'\nconst NOT_REVIEWED: &str = "{self.identity}";'
        self.assertNotIn(self.identity, CHECK.reviewed_identities(source))
        with self.assertRaises(CHECK.RuntimePackageError):
            CHECK.verify_package(self.runtime, source)

    def test_comment_and_string_lookalikes_do_not_supply_policy(self):
        fake = policy([self.identity])
        with self.assertRaises(CHECK.RuntimePackageError):
            CHECK.reviewed_identities("/* nested /* comment */ " + fake + " */")
        with self.assertRaises(CHECK.RuntimePackageError):
            CHECK.reviewed_identities('let example = r###"' + fake + '"###;')
        with self.assertRaises(CHECK.RuntimePackageError):
            CHECK.reviewed_identities('// ' + fake.replace("\n", "\n// "))
        self.assertEqual(CHECK.reviewed_identities('let example = r#"' + fake + '"#;\n' + self.source),
                         frozenset([self.identity]))

    def test_malformed_ambiguous_or_computed_policy_fails_closed(self):
        broken = [
            "", self.source.replace("[&str; 1]", "[&str; 2]"),
            self.source + "\n" + self.source,
            self.source.replace("[ENGINE_0]", "[UNKNOWN]"),
            self.source.replace("[&str; 1] = [ENGINE_0]", "[&str; 2] = [ENGINE_0, ENGINE_0]"),
            self.source.replace(f'"{self.identity}"', f'concat!("{self.identity}")'),
            self.source.replace(self.identity, self.identity.upper()),
            self.source.replace("[ENGINE_0]", f'["{self.identity}"]'),
            self.source.replace("[ENGINE_0]", "[ENGINE_0 + ENGINE_0]"),
            self.source.replace("const ENGINE_0", "const ENGINE_OTHER"),
            self.source + f'\nconst ENGINE_0: &str = "{self.identity}";',
            self.source + " /* unterminated",
        ]
        for source in broken:
            with self.subTest(source=source):
                with self.assertRaises(CHECK.RuntimePackageError):
                    CHECK.reviewed_identities(source)

    def test_external_links_and_special_files_are_refused(self):
        outside = self.root / "outside"
        outside.write_bytes(b"not part of engine")
        link = self.runtime / "external"
        link.symlink_to(outside)
        with self.assertRaises(CHECK.RuntimePackageError):
            CHECK.verify_package(self.runtime, self.source)
        link.unlink()
        os.mkfifo(self.runtime / "pipe")
        with self.assertRaises(CHECK.RuntimePackageError):
            CHECK.verify_package(self.runtime, self.source)


if __name__ == "__main__":
    unittest.main()
