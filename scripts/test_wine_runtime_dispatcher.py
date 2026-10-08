"""Test runtime-view routing with original fake executables, without Wine."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


REPO = Path(__file__).resolve().parent.parent


class SwitchableDispatcher(unittest.TestCase):
    def setUp(self):
        build = REPO / ".build"
        build.mkdir(exist_ok=True)
        self.root = Path(tempfile.mkdtemp(prefix="dispatcher-tests-", dir=build))
        self.package = self.root / "package with spaces"
        (self.package / "bin").mkdir(parents=True)
        (self.package / "lib").mkdir()
        (self.package / "lib/libfreetype.6.dylib").touch()
        self.base = self.package / "runtime"
        self.dxmt = self.package / "components/dxmt/runtime"
        for view, label in [(self.base, "base"), (self.dxmt, "dxmt")]:
            (view / "bin").mkdir(parents=True)
            (view / "lib/wine/x86_64-unix").mkdir(parents=True)
            (view / "lib/wine/x86_64-unix/ntdll.so").touch()
            for name in ["wine", "wineboot", "wineserver"]:
                program = view / "bin" / name
                program.write_text("#!/usr/bin/env python3\nimport json,os,sys\n"
                                   "print(json.dumps({'view':" + repr(label) + ","
                                   "'arguments':sys.argv[1:], 'environment':{k:os.environ.get(k) for k in "
                                   "['CASKRUN_GRAPHICS_BACKEND','CASKRUN_NATIVE_NTDLL','WINEDLLPATH',"
                                   "'WINEDLLOVERRIDES','WINELOADER','WINESERVER','WINEDATADIR']}}))\n")
                program.chmod(0o755)
        shutil.copyfile(REPO / "scripts/wine-runtime-launch.sh", self.package / "bin/_dispatch")
        (self.package / "bin/_dispatch").chmod(0o755)
        for name in ["wine", "wineboot", "wineserver"]:
            (self.package / "bin" / name).symlink_to("_dispatch")
        (self.package / "graphics-layout.txt").write_text("caskrun-switchable-graphics-v1\n")

    def tearDown(self):
        shutil.rmtree(self.root)

    def launch(self, command="wine", backend=None, **environment):
        clean = {key: value for key, value in os.environ.items()
                 if not key.startswith(("WINE", "CASKRUN_", "DYLD_"))}
        if backend is not None:
            clean["CASKRUN_GRAPHICS_BACKEND"] = backend
        clean.update(environment)
        return subprocess.run([str(self.package / "bin" / command), "argument with spaces", "second"],
                              env=clean, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def test_unset_auto_and_wined3d_use_base_view(self):
        for backend in [None, "auto", "wined3d"]:
            with self.subTest(backend=backend):
                result = self.launch(backend=backend)
                self.assertEqual(result.returncode, 0, result.stderr)
                data = json.loads(result.stdout)
                self.assertEqual(data["view"], "base")
                self.assertEqual(data["environment"]["CASKRUN_GRAPHICS_BACKEND"], "wined3d")
                self.assertEqual(data["environment"]["WINELOADER"], str(self.base / "bin/wine"))

    def test_dxmt_uses_its_real_view_for_all_loader_paths(self):
        for command in ["wine", "wineboot"]:
            with self.subTest(command=command):
                result = self.launch(command, "dxmt")
                self.assertEqual(result.returncode, 0, result.stderr)
                data = json.loads(result.stdout)
                self.assertEqual(data["view"], "dxmt")
                self.assertEqual(data["arguments"], ["argument with spaces", "second"])
                self.assertEqual(data["environment"]["WINELOADER"], str(self.dxmt / "bin/wine"))
                self.assertEqual(data["environment"]["WINESERVER"], str(self.base / "bin/wineserver"))
                self.assertEqual(data["environment"]["WINEDATADIR"], str(self.dxmt / "share/wine"))
                self.assertEqual(data["environment"]["WINEDLLPATH"], str(self.dxmt / "lib/wine"))

    def test_native_loader_is_remapped_only_between_the_exact_views(self):
        for input_view in [self.base, self.dxmt]:
            for backend, output_view in [("dxmt", self.dxmt), ("auto", self.base)]:
                with self.subTest(input=input_view, backend=backend):
                    result = self.launch(backend=backend,
                                         CASKRUN_NATIVE_NTDLL=str(input_view / "lib/wine/x86_64-unix/ntdll.so"))
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(json.loads(result.stdout)["environment"]["CASKRUN_NATIVE_NTDLL"],
                                     str(output_view / "lib/wine/x86_64-unix/ntdll.so"))
        result = self.launch(backend="dxmt", CASKRUN_NATIVE_NTDLL=str(self.root / "unrelated/ntdll.so"))
        self.assertEqual(result.returncode, 65)
        self.assertEqual(result.stdout, "")

    def test_unknown_backend_is_refused_before_execution(self):
        result = self.launch(backend="unknown")
        self.assertEqual(result.returncode, 64)
        self.assertEqual(result.stdout, "")

    def test_missing_or_wrong_layout_marker_refuses_dxmt(self):
        marker = self.package / "graphics-layout.txt"
        marker.unlink()
        self.assertEqual(self.launch(backend="dxmt").returncode, 65)
        self.assertEqual(self.launch(backend="auto").returncode, 0)
        marker.write_text("unreviewed-layout\n")
        self.assertEqual(self.launch(backend="dxmt").returncode, 65)

    def test_incomplete_selected_view_is_refused(self):
        (self.dxmt / "bin/wineboot").unlink()
        result = self.launch("wineboot", "dxmt")
        self.assertEqual(result.returncode, 66)
        self.assertEqual(result.stdout, "")

    def test_server_control_always_uses_base_even_with_invalid_backend(self):
        for backend in ["dxmt", "unknown"]:
            result = self.launch("wineserver", backend,
                                 CASKRUN_NATIVE_NTDLL=str(self.root / "unrelated/ntdll.so"))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout)["view"], "base")

    def test_custom_dll_overrides_and_additional_paths_are_preserved(self):
        result = self.launch(backend="wined3d", WINEDLLOVERRIDES="example=n,b", WINEDLLPATH="extra-search-path")
        self.assertEqual(result.returncode, 0, result.stderr)
        environment = json.loads(result.stdout)["environment"]
        self.assertEqual(environment["WINEDLLOVERRIDES"], "example=n,b")
        self.assertEqual(environment["WINEDLLPATH"], str(self.base / "lib/wine") + ":extra-search-path")


if __name__ == "__main__":
    unittest.main()
