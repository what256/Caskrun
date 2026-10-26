"""Local packaging owner guards using opened dummy files; never an app or Wine."""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile
import unittest


PROJECT = Path(__file__).resolve().parent.parent
GUARD = PROJECT / "scripts/assert-app-idle.sh"


class AppIdleGuards(unittest.TestCase):
    def setUp(self):
        build = PROJECT / ".build"
        build.mkdir(exist_ok=True)
        self.root = Path(tempfile.mkdtemp(prefix="app-idle-tests-", dir=build))
        self.app = self.root / "Fixture.app"
        self.executables = self.app / "Contents/MacOS"
        self.executables.mkdir(parents=True)
        for name in ("Caskrun", "caskrun-core", "caskrun-native-icon"):
            (self.executables / name).write_bytes(b"nonexecutable fixture only")
        self.runtime = self.app / "Contents/Resources/wine-runtime"
        self.runtime.mkdir(parents=True)
        (self.runtime / "engine").write_bytes(b"not a real engine")

    def tearDown(self):
        # No fixture executable is ever started. All with-open handles and
        # synchronous guard subprocesses have finished before this removal.
        shutil.rmtree(self.root)

    def guard(self, app=None):
        return subprocess.run(["/bin/zsh", str(GUARD), str(app or self.app)],
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              timeout=20, check=False)

    def test_absent_app_and_idle_regular_components_pass(self):
        self.assertEqual(self.guard(self.root / "Absent.app").returncode, 0)
        self.assertEqual(self.guard().returncode, 0)

    def test_each_owned_executable_prevents_replacement(self):
        for name in ("Caskrun", "caskrun-core", "caskrun-native-icon"):
            with self.subTest(component=name):
                with (self.executables / name).open("rb"):
                    result = self.guard()
                    self.assertEqual(result.returncode, 75)
                    self.assertIn(b"Refusing to replace an app in use", result.stderr)
                self.assertEqual(self.guard().returncode, 0)

    def test_runtime_file_owner_prevents_replacement(self):
        with (self.runtime / "engine").open("rb"):
            self.assertEqual(self.guard().returncode, 75)
        self.assertEqual(self.guard().returncode, 0)

    def test_missing_optional_helper_keeps_older_apps_supported(self):
        (self.executables / "caskrun-native-icon").unlink()
        self.assertEqual(self.guard().returncode, 0)

    def test_linked_app_or_component_is_refused(self):
        link = self.root / "Alias.app"
        link.symlink_to(self.app, target_is_directory=True)
        self.assertEqual(self.guard(link).returncode, 75)
        file = self.executables / "caskrun-core"
        file.unlink()
        file.symlink_to(self.runtime / "engine")
        self.assertEqual(self.guard().returncode, 75)
        file.unlink()
        file.symlink_to(self.root / "Missing")
        self.assertEqual(self.guard().returncode, 75)

    def test_linked_parent_directory_is_refused(self):
        real = self.root / "real-executables"
        self.executables.rename(real)
        self.executables.symlink_to(real, target_is_directory=True)
        self.assertEqual(self.guard().returncode, 75)
        self.executables.unlink()
        real.rename(self.executables)
        parent = self.root / "alias-parent"
        parent.symlink_to(self.root, target_is_directory=True)
        self.assertEqual(self.guard(parent / self.app.name).returncode, 75)

    def test_special_or_wrong_component_type_is_refused(self):
        file = self.executables / "caskrun-core"
        file.unlink()
        os.mkfifo(file)
        self.assertEqual(self.guard().returncode, 75)
        file.unlink()
        file.mkdir()
        self.assertEqual(self.guard().returncode, 75)


if __name__ == "__main__":
    unittest.main()
