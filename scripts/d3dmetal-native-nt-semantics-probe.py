#!/usr/bin/env python3
"""Exercise the original optional NT ABI boundary in an owned Wine fixture.

The selected module is a clearly named mock support dylib, never Apple code.
It implements the two published region-support contracts solely to trigger the
loader boundary. Its callback verifies real Wine event, semaphore, suspended
thread, termination and close semantics from both calling conventions.
"""

from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
WORK = ROOT / ".build/d3dmetal-bridge-integration"
TEST = WORK / "nt-semantics-test"


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    sys.modules[name] = result
    spec.loader.exec_module(result)
    return result


def main():
    abi = module("d3dmetal_abi_contract", ROOT / "scripts/d3dmetal-native-abi-probe.py")
    runner = module("d3dmetal_nt_owned_session", ROOT / "scripts/test-dxmt-d3d12-dev.py")
    builder = module("d3dmetal_nt_builder", ROOT / "scripts/build-wine-steam-reopen-dev.py")
    TEST.mkdir(exist_ok=True)
    header = '''#include <stdint.h>
#include <stdbool.h>
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
typedef int32_t NTSTATUS, LONG;
typedef uint32_t ULONG, ACCESS_MASK, EVENT_TYPE;
typedef uint8_t BOOLEAN;
typedef uintptr_t ULONG_PTR, SIZE_T;
typedef void *HANDLE;
typedef struct object_attributes OBJECT_ATTRIBUTES;
typedef struct attribute_list PS_ATTRIBUTE_LIST;
typedef NTSTATUS (*PRTL_THREAD_START_ROUTINE)(void *);
'''
    declarations = []
    for name, signature in abi.CONTRACTS:
        declarations.append(f"typedef NTSTATUS (CALL_ABI *fn_{name})({signature});\n"
                            f"fn_{name} p_{name} = (fn_{name})dlsym(RTLD_DEFAULT, \"{name}\");\n"
                            f"if (!p_{name}) return {len(declarations) + 1};")
    function = '''
int RUN_TEST(void) {
''' + "\n".join(declarations) + '''
    typedef NTSTATUS (*terminate_fn)(HANDLE, NTSTATUS);
    typedef NTSTATUS (*wait_fn)(HANDLE, BOOLEAN, const int64_t *);
    terminate_fn terminate = (terminate_fn)dlsym(RTLD_DEFAULT, "NtTerminateThread");
    wait_fn wait = (wait_fn)dlsym(RTLD_DEFAULT, "NtWaitForSingleObject");
    HANDLE event = NULL, semaphore = NULL, thread = NULL;
    LONG previous = -1;
    ULONG semaphore_previous = 0;
    int64_t timeout = -10000000;
    int failure = 0;
    if (!terminate || !wait) return 10;
#define CHECK(expression, code) do { if (!(expression)) { failure = code; goto cleanup; } } while (0)
    CHECK(p_NtCreateEvent(&event, 0x1f0003, NULL, 0, 0) == 0 && event, 11);
    CHECK(p_NtSetEvent(event, &previous) == 0 && previous == 0, 12);
    CHECK(p_NtResetEvent(event, &previous) == 0 && previous == 1, 13);
    CHECK(p_NtClearEvent(event) == 0, 14);
    CHECK(p_NtPulseEvent(event, &previous) == 0 && previous == 0, 15);
    CHECK(p_NtCreateSemaphore(&semaphore, 0x1f0003, NULL, 1, 3) == 0 && semaphore, 16);
    CHECK(p_NtReleaseSemaphore(semaphore, 1, &semaphore_previous) == 0 && semaphore_previous == 1, 17);
    /* Never run a native pointer as PE code: this thread starts suspended and
     * is terminated and waited for before closing its owned handle. */
    CHECK(p_NtCreateThreadEx(&thread, 0x1fffff, NULL, (HANDLE)(intptr_t)-1,
          NULL, NULL, 1, 0, 0x10000, 0x100000, NULL) == 0 && thread, 18);
    CHECK(terminate(thread, 0) == 0, 19);
    CHECK(wait(thread, 0, &timeout) == 0, 20);
cleanup:
    if (thread) {
        terminate(thread, 0);
        if (p_NtClose(thread) != 0 && !failure) failure = 21;
    }
    if (event && p_NtClose(event) != 0 && !failure) failure = 22;
    if (semaphore && p_NtClose(semaphore) != 0 && !failure) failure = 23;
    return failure;
}
'''
    (TEST / "nt-contract.h").write_text(header + function)
    (TEST / "sysv-caller.c").write_text('#define CALL_ABI\n#define RUN_TEST run_sysv_semantics\n#include "nt-contract.h"\n')
    (TEST / "mock-region-support.c").write_text('''#define CALL_ABI __attribute__((ms_abi))
#define RUN_TEST run_ms_semantics
#include "nt-contract.h"
bool supports_non_native_code_regions(void) { return true; }
void register_non_native_code_region(void *start, void *end) {
    static bool entered;
    (void)start; (void)end;
    if (entered) return;
    entered = true;
    const char *caller_path = getenv("CASKRUN_D3DMETAL_TEST_SYSV_CALLER");
    const char *output_path = getenv("CASKRUN_D3DMETAL_TEST_RESULT");
    void *caller = caller_path ? dlopen(caller_path, RTLD_NOW | RTLD_LOCAL) : NULL;
    int (*run_sysv)(void) = caller ? dlsym(caller, "run_sysv_semantics") : NULL;
    int ms_status = run_ms_semantics();
    int sysv_status = run_sysv ? run_sysv() : 99;
    FILE *output = output_path ? fopen(output_path, "wx") : NULL;
    if (output) {
        fprintf(output, "{\\"passed\\":%s,\\"real_nt_signatures\\":9,\\"msabi_status\\":%d,\\"sysv_status\\":%d,\\"thread_lifecycle\\":\\"suspended_terminated_waited_closed\\",\\"mock_support_only\\":true}\\n",
                !ms_status && !sysv_status ? "true" : "false", ms_status, sysv_status);
        fclose(output);
    }
}
''')
    (TEST / "main.c").write_text('#include <stdio.h>\nint main(void) { puts("{\\"fixture_main_reached\\":true}"); return 0; }\n')
    flags = ["clang", "-arch", "x86_64", "-std=gnu11", "-O2", "-Wall", "-Wextra", "-Werror",
             "-fno-optimize-sibling-calls", "-dynamiclib"]
    for source, binary in (("sysv-caller.c", "libsysv-caller.dylib"),
                           ("mock-region-support.c", "libmock-region-support.dylib")):
        subprocess.run(flags + [source, "-o", binary], cwd=TEST, check=True, timeout=30)
    subprocess.run(["x86_64-w64-mingw32-gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                    "main.c", "-o", "nt-semantics.exe"], cwd=TEST, check=True, timeout=30)
    package = WORK / "private-runtime"
    runner.harness.real_path(package, directory=True)
    table = runner.harness.process_table()
    runner.require(not any(str(package) in row["command"] for pid, row in table.items()
                           if pid != os.getpid()),
                   "Private candidate has a process owner; no image was replaced")
    inspection = subprocess.run(["/usr/sbin/lsof", "-nP", "-Fpc", "+D", str(package)],
                                capture_output=True, timeout=30)
    runner.require(inspection.returncode == 1 and not inspection.stdout and not inspection.stderr,
                   "Private candidate idleness was not established; no image was replaced")
    target = package / "runtime/lib/wine/x86_64-unix/ntdll.so"
    shutil.copyfile(WORK / "engine/base-build/dlls/ntdll/ntdll.so", target)
    builder.PACKAGE.repair_image(package, target)
    runner.EXPERIMENT = TEST
    session = runner.Session(package)
    (TEST / "active-session.json").write_text(json.dumps({"root": str(session.root), "prefix": str(session.prefix)}) + "\n")
    result = {"passed": False, "mock_support_only": True}
    try:
        wine = package / "bin/wine"
        env = session.environment("wined3d")
        code, _ = session.invoke([wine, "wineboot.exe", "-u"], env, "bootstrap", 60)
        runner.require(code == 0, "Private Wine bootstrap failed")
        for key, value in (("Auto", "1"), ("Debugger", "false.exe")):
            code, _ = session.invoke([wine, "reg.exe", "add", "HKLM\\Software\\Microsoft\\Windows NT\\CurrentVersion\\AeDebug",
                                     "/v", key, "/t", "REG_SZ", "/d", value, "/f"], env, "debugger-" + key, 10)
            runner.require(code == 0, "Could not suppress fixture crash UI")
        session.stop_and_check_idle()
        fixture = session.root / "app/nt-semantics.exe"
        shutil.copyfile(TEST / "nt-semantics.exe", fixture)
        output = session.root / "nt-semantics.json"
        env.update(CASKRUN_D3DMETAL_SHARED_LIBRARY=str(TEST / "libmock-region-support.dylib"),
                   CASKRUN_D3DMETAL_TEST_SYSV_CALLER=str(TEST / "libsysv-caller.dylib"),
                   CASKRUN_D3DMETAL_TEST_RESULT=str(output))
        code, data = session.invoke([wine, fixture], env, "nt-semantics", 30)
        runner.require(output.is_file(), "Mock region callback did not finish its NT test")
        result = json.loads(output.read_text())
        result["exit_code"] = code
        result["fixture_main_reached"] = b'"fixture_main_reached":true' in data
        result["passed"] = result["passed"] and code == 0 and result["fixture_main_reached"]
    except BaseException as error:
        result["error"] = str(error)
    finally:
        try:
            session.stop_and_check_idle()
            session._validate_owner()
            runner.require(shutil.rmtree.avoids_symlink_attacks, "Safe fixture removal is unavailable")
            shutil.rmtree(session.root)
            result["cleanup"] = "removed_after_no_owners_verified"
        except BaseException as error:
            result["cleanup_error"] = str(error)
            result["passed"] = False
        result["passed"] = result["passed"] and result.get("cleanup") == "removed_after_no_owners_verified"
        (TEST / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        print(json.dumps(result))
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
