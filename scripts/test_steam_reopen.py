#!/usr/bin/env python3
"""Exercise the exact native Steam Dock-reopen request with inert original fixtures.

Run against one or more isolated Wine source views inside .build (for example
the base and DXMT driver trees) to which wine-11.0-steam-reopen.patch has been
applied after the Steam Quit patch. No Wine, publisher executable, user bottle,
account or system process is used. --interactive briefly shows this test's own
window.
"""
from pathlib import Path
import argparse
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--source", type=Path, nargs="+", required=True)
parser.add_argument("--interactive", action="store_true")
ARGS, TEST_ARGS = parser.parse_known_args()
SOURCES = [source.resolve() for source in ARGS.source]
for source in SOURCES:
    if not source.is_relative_to((REPO / ".build").resolve()):
        parser.error("each source must be an isolated tree inside .build")

CLANG = ["/usr/bin/clang", "-Wall", "-Wextra", "-Werror", "-Wno-unused-parameter", "-Wno-deprecated-declarations"]
STATE = (
    "static BOOL caskrunSteamShutdownPending;",
    "static BOOL caskrunSteamShowPending, caskrunSteamQuitQueued, caskrunSteamQuitHeld;",
    "static unsigned long caskrunSteamQuitGeneration;",
    "enum caskrun_steam_control { CASKRUN_STEAM_CONTROL_QUIT, CASKRUN_STEAM_CONTROL_SHOW };",
    "static void CaskrunSteamControlFinished(enum caskrun_steam_control, BOOL);",
)
SPAWN_HELPERS = (
    "static BOOL CaskrunOwnedProgram(", "static BOOL CaskrunSteamQuitDirectory(",
    "static BOOL CaskrunSteamQuitBindings(", "static BOOL CaskrunSteamQuitKeepEnvironment(",
    "static BOOL CaskrunSpawnSteamControl(", "static BOOL CaskrunSubmitSteamQuit(",
    "static void CaskrunSteamControlFinished(enum caskrun_steam_control control, BOOL failed)",
    "static BOOL CaskrunRequestSteamShutdown(void)",
)
PROCESS = (
    "static BOOL CaskrunSteamDockEnabled(", "static NSString* CaskrunArgumentBasename(",
    "static BOOL CaskrunIsWindowsProcess(", "static BOOL CaskrunNativeDockEnabled(",
    "static NSString* CaskrunNativeImageName(", "static BOOL CaskrunNativeBackgroundProcess(",
    "static BOOL CaskrunPreparedSteamUI(",
)
CONTROLLER = (
    "- (BOOL) isAnyWineWindowVisible", "- (void) unminimizeWindowIfNoneVisible",
    "- (void) caskrunRememberPresentedSteamWindow:",
    "- (BOOL) caskrunHasRestorableSteamWindow", "- (BOOL) applicationShouldHandleReopen:",
)
PRESENTATION_STATE = "static BOOL caskrunSteamPresentedClient;"

# Shared by the reused Quit template and the new show path. The expected verb
# defaults to Quit so the original Quit template runs unchanged.
CHILD = r'''
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
int main(int argc,char **argv){
 const char *verb=getenv("CASKRUN_FIXTURE_VERB");if(!verb)verb="request-steam-quit";
 assert(argc==3&&!strcmp(argv[1],verb)&&!strcmp(argv[2],"11111111-1111-4111-8111-111111111111"));
 assert(!strcmp(getenv("CASKRUN_HOME"),getenv("CASKRUN_QUIT_FIXTURE_DATA")));
 assert(!strcmp(getenv("WINEPREFIX"),getenv("CASKRUN_QUIT_FIXTURE_PREFIX")));
 assert(!getenv("CASKRUN_NATIVE_CORE")&&!getenv("CASKRUN_NATIVE_EXEC")&&!getenv("CASKRUN_NATIVE_DATA")&&!getenv("CASKRUN_NATIVE_BOTTLE")&&!getenv("CASKRUN_NATIVE_IMAGE"));
 assert(!getenv("WINESERVERSOCKET")&&!getenv("WINELOADERNOEXEC")&&!getenv("WINEPRELOADRESERVE")&&!getenv("CASKRUN_STEAM_MODE")&&!getenv("CASKRUN_STEAM_NATIVE_APP")&&!getenv("CASKRUN_STEAM_ICON"));
 assert(!strcmp(getenv("CASKRUN_QUIT_FIXTURE_KEEP"),"present"));char cwd[PATH_MAX];assert(getcwd(cwd,sizeof(cwd))&&!strcmp(cwd,"/"));
 struct stat expected,actual;assert(!stat("/dev/null",&expected));for(int fd=0;fd<3;fd++){assert(!fstat(fd,&actual));assert(S_ISCHR(actual.st_mode)&&actual.st_rdev==expected.st_rdev);assert((fcntl(fd,F_GETFL)&O_ACCMODE)==(fd?O_WRONLY:O_RDONLY));}
 errno=0;assert(fcntl(atoi(getenv("CASKRUN_QUIT_FIXTURE_FD")),F_GETFD)==-1&&errno==EBADF);
 FILE *report=fopen(getenv("CASKRUN_QUIT_FIXTURE_REPORT"),"wx");assert(report);fputs("verified\n",report);assert(!fclose(report));return 0;
}
'''


def extract(text, signature):
    start = text.index(signature)
    position = text.index("{", start)
    depth = 1
    end = position + 1
    while depth:
        depth += (text[end] == "{") - (text[end] == "}")
        end += 1
    return text[start:end]


def driver(source, name):
    return (source / "dlls/winemac.drv" / name).read_text()


def helpers(app, names):
    for line in STATE:
        if app.count("\n" + line + "\n") != 1:
            raise AssertionError("shared state differs from the fixture: " + line)
    return "\n".join(STATE) + "\n" + "\n".join(extract(app, name) for name in names)


def build_child(root):
    source = root / "child.c"
    source.write_text(CHILD)
    child = root / "fixture-core"
    subprocess.run(["/usr/bin/clang", "-Wall", "-Wextra", "-Werror", str(source), "-o", str(child)], check=True)
    child.chmod(0o700)
    return child


class SteamReopenTests(unittest.TestCase):
    def test_views_share_identical_reopen_source(self):
        # DXMT patches change macdrv.h, macdrv_cocoa.h and window.c only, so
        # both driver views must carry byte-identical Dock-reopen code.
        apps = {driver(source, "cocoa_app.m") for source in SOURCES}
        self.assertEqual(len(apps), 1)

    def test_reopen_is_scoped_and_never_orders_windows(self):
        for source in SOURCES:
            with self.subTest(source=str(source)):
                app = driver(source, "cocoa_app.m")
                reopen = extract(app, "- (BOOL) applicationShouldHandleReopen:")
                decided = reopen.index("BOOL steamNeedsShow =")
                self.assertLess(decided, reopen.index("[self unminimizeWindowIfNoneVisible];"))
                self.assertIn("CaskrunPreparedSteamUI() && caskrunSteamPresentedClient &&",
                              reopen[decided:reopen.index(";", decided)])
                self.assertIn("![self caskrunHasRestorableSteamWindow]",
                              reopen[decided:reopen.index(";", decided)])
                self.assertIn("if (steamNeedsShow)", reopen)
                for forbidden in ("orderFront", "makeKeyAndOrderFront", "orderBelow", "orderWindow", "[NSApp unhide",
                                  "postEvent", "CaskrunRequestSteamShutdown", "setActivationPolicy", "isHidden"):
                    self.assertNotIn(forbidden, reopen)
                restorable = extract(app, "- (BOOL) caskrunHasRestorableSteamWindow")
                for rule in ("[window isKindOfClass:[WineWindow class]]", "!window.closing",
                             "[window presentsVisibleContent]", "[window wouldBeVisible] || [window isMiniaturized]"):
                    self.assertIn(rule, restorable)
                self.assertIn("@interface WineWindow (CaskrunSteamReopen)\n- (BOOL) wouldBeVisible;\n- (BOOL) closing;\n- (BOOL) drawnSinceShown;\n@end", app)
                self.assertEqual(app.count("\n" + PRESENTATION_STATE + "\n"), 1)
                # One definition and one Dock call site; Core owns the client argument.
                self.assertEqual(app.count("CaskrunRequestSteamShow("), 2)
                self.assertEqual(reopen.count("CaskrunRequestSteamShow();"), 1)
                self.assertEqual(app.count('"request-steam-show"'), 1)
                self.assertNotIn("steam://", app)
                prepared = extract(app, "static BOOL CaskrunPreparedSteamUI(")
                for gate in ("CaskrunSteamDockEnabled()", "CaskrunNativeDockEnabled()",
                             'CaskrunIsWindowsProcess(@"steamwebhelper.exe")', "!CaskrunNativeBackgroundProcess()"):
                    self.assertIn(gate, prepared)
                show = extract(app, "static BOOL CaskrunRequestSteamShow(void)")
                guard = show[:show.index("CaskrunSpawnSteamControl(")]
                for intent in ("caskrunSteamShutdownPending", "caskrunSteamQuitQueued", "caskrunSteamQuitHeld",
                               "caskrunSteamShowPending"):
                    self.assertIn(intent, guard)
                finished = extract(app, "static void CaskrunSteamControlFinished(enum caskrun_steam_control control, BOOL failed)")
                # Only a same-generation timer ends the hold; nothing is cancelled or signalled.
                self.assertIn("if (generation == caskrunSteamQuitGeneration) caskrunSteamQuitHeld = NO;", finished)
                self.assertEqual(finished.count("dispatch_after("), 1)
                for forbidden in ("kill(", "SIGTERM", "terminate:", "_cancel(", "posix_spawn("):
                    self.assertNotIn(forbidden, finished)

    def test_presentation_history_is_captured_before_orderout_resets_it(self):
        for source in SOURCES:
            with self.subTest(source=str(source)):
                app, window = driver(source, "cocoa_app.m"), driver(source, "cocoa_window.m")
                callback = extract(app, "- (void) windowWillOrderOut:")
                self.assertLess(callback.index("[self caskrunRememberPresentedSteamWindow:window];"),
                                callback.index("[caskrunSteamHeaderRouter forgetWindow:window];"))
                orderout = extract(window, "- (void) doOrderOut")
                capture = orderout.index("[controller windowWillOrderOut:self];")
                self.assertLess(capture, orderout.index("drawnSinceShown = NO;"))
                self.assertLess(capture, orderout.index("savedVisibleState = FALSE;"))

    def test_close_and_quit_semantics_are_unchanged(self):
        for source in SOURCES:
            with self.subTest(source=str(source)):
                app, window = driver(source, "cocoa_app.m"), driver(source, "cocoa_window.m")
                close = extract(window, "- (void) caskrunCloseSteamLogin:")
                self.assertIn("[NSApp hide:", close)
                should_close = extract(window, "- (BOOL)windowShouldClose:")
                # Other windows and stores still send Windows their close request.
                self.assertIn("WINDOW_CLOSE_REQUESTED", should_close)
                quit = extract(app, "- (NSApplicationTerminateReply) applicationShouldTerminate:")
                self.assertIn("CaskrunRequestSteamShutdown()", quit)
                for text in (close, should_close, quit):
                    self.assertNotIn("CaskrunRequestSteamShow", text)

    def test_original_quit_fixture_still_passes_through_shared_spawner(self):
        template = (REPO / "scripts/fixtures/steam-quit.m.in").read_text()
        for source in SOURCES:
            with self.subTest(source=str(source)):
                app = driver(source, "cocoa_app.m")
                code = template.replace("@@HELPERS@@", helpers(app, SPAWN_HELPERS)).replace(
                    "@@DELEGATE@@", extract(app, "- (NSApplicationTerminateReply) applicationShouldTerminate:"))
                with tempfile.TemporaryDirectory(prefix=".steam-reopen-quit-test-", dir=REPO / ".build") as temporary:
                    root = Path(temporary).resolve()
                    child = build_child(root)
                    program = root / "fixture.m"
                    program.write_text(code)
                    binary = root / "native-quit-test"
                    subprocess.run([*CLANG, "-framework", "AppKit", "-framework", "Carbon", str(program), "-o", str(binary)], check=True)
                    result = subprocess.run([str(binary), str(root), str(child)], capture_output=True, text=True, timeout=20)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn("exact native Quit guards, process lifecycle and delegate checks passed", result.stdout)
                    print(result.stdout.strip())

    def test_show_request_and_reopen_delegate(self):
        template = (REPO / "scripts/fixtures/steam-reopen.m.in").read_text()
        for source in SOURCES:
            with self.subTest(source=str(source)):
                app = driver(source, "cocoa_app.m")
                code = (template.replace("@@HELPERS@@", helpers(app, (*SPAWN_HELPERS, "static BOOL CaskrunRequestSteamShow(void)")))
                        .replace("@@PRESENTATION_STATE@@", PRESENTATION_STATE)
                        .replace("@@PROCESS@@", "\n".join(extract(app, name) for name in PROCESS))
                        .replace("@@CONTROLLER@@", "\n".join(extract(app, name) for name in CONTROLLER)))
                with tempfile.TemporaryDirectory(prefix=".steam-reopen-test-", dir=REPO / ".build") as temporary:
                    root = Path(temporary).resolve()
                    child = build_child(root)
                    program = root / "fixture.m"
                    program.write_text(code)
                    binary = root / "native-reopen-test"
                    subprocess.run([*CLANG, "-framework", "AppKit", str(program), "-o", str(binary)], check=True)
                    command = [str(binary), str(root), str(child)] + (["--interactive"] if ARGS.interactive else [])
                    result = subprocess.run(command, capture_output=True, text=True, timeout=60)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn("native Steam Dock-reopen request, delegate and scope checks passed", result.stdout)
                    print(result.stdout.strip())


if __name__ == "__main__":
    unittest.main(argv=[__file__, *TEST_ARGS])
