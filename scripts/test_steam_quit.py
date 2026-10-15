#!/usr/bin/env python3
"""Exercise exact native Steam Quit helpers with inert original fixtures.

No Wine, user bottle, native UI, account, clipboard or system process is used.
"""
from pathlib import Path
import argparse
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--source", type=Path, default=REPO / ".build/native-text-runtime-v12/base-build/src/wine-11.0")
ARGS, TEST_ARGS = parser.parse_known_args()
SOURCE = ARGS.source.resolve()
if not SOURCE.is_relative_to((REPO / ".build").resolve()):
    parser.error("source must be an isolated tree inside .build")


def extract(text, signature):
    start = text.index(signature)
    position = text.index("{", start)
    depth = 1
    end = position + 1
    while depth:
        depth += (text[end] == "{") - (text[end] == "}")
        end += 1
    return text[start:end]


CHILD = r'''
#include <assert.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
int main(int argc,char **argv){
 assert(argc==3&&!strcmp(argv[1],"request-steam-quit")&&!strcmp(argv[2],"11111111-1111-4111-8111-111111111111"));
 assert(!strcmp(getenv("CASKRUN_HOME"),getenv("CASKRUN_QUIT_FIXTURE_DATA")));
 assert(!strcmp(getenv("WINEPREFIX"),getenv("CASKRUN_QUIT_FIXTURE_PREFIX")));
 assert(!getenv("CASKRUN_NATIVE_CORE")&&!getenv("CASKRUN_NATIVE_EXEC")&&!getenv("CASKRUN_NATIVE_DATA")&&!getenv("CASKRUN_NATIVE_BOTTLE"));
 assert(!getenv("WINESERVERSOCKET")&&!getenv("WINELOADERNOEXEC")&&!getenv("WINEPRELOADRESERVE")&&!getenv("CASKRUN_STEAM_MODE")&&!getenv("CASKRUN_STEAM_NATIVE_APP")&&!getenv("CASKRUN_STEAM_ICON"));
 assert(!strcmp(getenv("CASKRUN_QUIT_FIXTURE_KEEP"),"present"));char cwd[PATH_MAX];assert(getcwd(cwd,sizeof(cwd))&&!strcmp(cwd,"/"));
 struct stat expected,actual;assert(!stat("/dev/null",&expected));for(int fd=0;fd<3;fd++){assert(!fstat(fd,&actual));assert(S_ISCHR(actual.st_mode)&&actual.st_rdev==expected.st_rdev);assert((fcntl(fd,F_GETFL)&O_ACCMODE)==(fd?O_WRONLY:O_RDONLY));}
 errno=0;assert(fcntl(atoi(getenv("CASKRUN_QUIT_FIXTURE_FD")),F_GETFD)==-1&&errno==EBADF);
 FILE *report=fopen(getenv("CASKRUN_QUIT_FIXTURE_REPORT"),"wx");assert(report);fputs("verified\n",report);assert(!fclose(report));return 0;
}
'''


class SteamQuitTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = (SOURCE / "dlls/winemac.drv/cocoa_app.m").read_text()

    def test_exact_helpers_spawn_and_delegate_without_wine_or_ui(self):
        names = ("static BOOL CaskrunOwnedProgram", "static BOOL CaskrunSteamQuitDirectory",
                 "static BOOL CaskrunSteamQuitBindings", "static BOOL CaskrunSteamQuitKeepEnvironment",
                 "static BOOL CaskrunRequestSteamShutdown")
        helpers = "static BOOL caskrunSteamShutdownPending;\n" + "\n".join(extract(self.app, name) for name in names)
        template = (REPO / "scripts/fixtures/steam-quit.m.in").read_text()
        code = template.replace("@@HELPERS@@", helpers).replace("@@DELEGATE@@", extract(self.app, "- (NSApplicationTerminateReply) applicationShouldTerminate:"))
        with tempfile.TemporaryDirectory(prefix=".steam-quit-test-", dir=REPO / ".build") as temporary:
            root = Path(temporary).resolve()
            child_source = root / "child.c"
            child_source.write_text(CHILD)
            child = root / "fixture-core"
            subprocess.run(["/usr/bin/clang", "-Wall", "-Wextra", "-Werror", str(child_source), "-o", str(child)], check=True)
            child.chmod(0o700)
            source = root / "fixture.m"
            source.write_text(code)
            binary = root / "native-quit-test"
            subprocess.run(["/usr/bin/clang", "-Wall", "-Wextra", "-Werror", "-Wno-unused-parameter", "-Wno-deprecated-declarations",
                            "-framework", "AppKit", "-framework", "Carbon", str(source), "-o", str(binary)], check=True)
            result = subprocess.run([str(binary), str(root), str(child)], capture_output=True, text=True, timeout=20)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("exact native Quit guards, process lifecycle and delegate checks passed", result.stdout)
            print(result.stdout.strip())

    def test_window_close_still_only_hides(self):
        window = (SOURCE / "dlls/winemac.drv/cocoa_window.m").read_text()
        close = extract(window, "- (void) caskrunCloseSteamLogin:")
        self.assertIn("[NSApp hide:", close)
        self.assertNotIn("CaskrunRequestSteamShutdown", close)
        self.assertNotIn("WINDOW_CLOSE_REQUESTED", close)
        quit_delegate = extract(self.app, "- (NSApplicationTerminateReply) applicationShouldTerminate:")
        managed = quit_delegate[quit_delegate.index("BOOL requested ="):quit_delegate.index("[eventQueuesLock lock]")]
        self.assertIn("return NSTerminateCancel;", managed)
        self.assertNotIn("postEvent", managed)

    def test_exact_failure_notice_is_bounded_without_native_ui(self):
        template = (REPO / "scripts/fixtures/steam-quit-alert.m.in").read_text()
        code = template.replace("@@FAILURE@@", extract(self.app, "static void CaskrunSteamQuitFailed"))
        with tempfile.TemporaryDirectory(prefix=".steam-quit-alert-test-", dir=REPO / ".build") as temporary:
            root = Path(temporary)
            source = root / "notice.m"
            source.write_text(code)
            binary = root / "notice-test"
            subprocess.run(["/usr/bin/clang", "-Wall", "-Wextra", "-Werror", "-Wno-unused-parameter",
                            "-framework", "Foundation", str(source), "-o", str(binary)], check=True)
            result = subprocess.run([str(binary)], text=True, capture_output=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("duplicate suppression", result.stdout)
            print(result.stdout.strip())


if __name__ == "__main__":
    unittest.main(argv=[__file__, *TEST_ARGS])
