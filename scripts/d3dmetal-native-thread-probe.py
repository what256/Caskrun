#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Prepare, or explicitly run, an owned native MSABI thread-start contract.

The mock support module is original test code, not Apple code. Both callers
use a real Wine thread, created suspended then resumed. The native MSABI start
function exercises normal public pthread TLS and signals only its own event.
No private TLS inspection, manual segment changes, stores or user bottles.
Without --run this builds the fixture only and never launches Wine.
"""
from __future__ import annotations

import argparse
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
TEST = WORK / "native-thread-test"


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    sys.modules[name] = value
    spec.loader.exec_module(value)
    return value


SOURCE = r'''#include <dlfcn.h>
#include <pthread.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <fcntl.h>
#include <limits.h>
#include <sys/mman.h>
typedef int32_t NTSTATUS, LONG;
typedef uint32_t ULONG, ACCESS_MASK;
typedef uint8_t BOOLEAN;
typedef uintptr_t ULONG_PTR, SIZE_T;
typedef void *HANDLE;
typedef struct object_attributes OBJECT_ATTRIBUTES;
typedef struct attribute_list PS_ATTRIBUTE_LIST;
typedef NTSTATUS (__attribute__((ms_abi)) *start_fn)(void *);
typedef NTSTATUS (CALL_ABI *create_event_fn)(HANDLE *, ACCESS_MASK, const OBJECT_ATTRIBUTES *, ULONG, BOOLEAN);
/* This callback is always inside the explicitly selected mock module, even
 * when an unrelated SysV caller creates the thread. */
typedef NTSTATUS (__attribute__((ms_abi)) *set_event_fn)(HANDLE, LONG *);
typedef NTSTATUS (CALL_ABI *close_fn)(HANDLE);
typedef NTSTATUS (CALL_ABI *create_thread_fn)(HANDLE *, ACCESS_MASK, OBJECT_ATTRIBUTES *, HANDLE,
    start_fn, void *, ULONG, ULONG_PTR, SIZE_T, SIZE_T, PS_ATTRIBUTE_LIST *);
typedef NTSTATUS (*resume_fn)(HANDLE, ULONG *);
typedef NTSTATUS (*wait_fn)(HANDLE, BOOLEAN, const int64_t *);
typedef NTSTATUS (*terminate_fn)(HANDLE, NTSTATUS);
typedef NTSTATUS (*query_fn)(HANDLE, ULONG, void *, ULONG, ULONG *);
/* Public Wine/NT THREAD_BASIC_INFORMATION interface, not Apple private data. */
struct thread_basic_info {
    NTSTATUS exit_status;
    void *teb;
    struct {void *process, *thread;} client_id;
    ULONG_PTR affinity;
    LONG priority, base_priority;
};
_Static_assert(sizeof(struct thread_basic_info) == 48, "public x64 NT layout");
_Static_assert(offsetof(struct thread_basic_info, exit_status) == 0, "exit status layout");

struct start_context {
    HANDLE event;
    set_event_fn set_event;
    pthread_key_t key;
    pid_t process;
    unsigned marker;
    unsigned entered, native_call_ok, tls_ok;
    NTSTATUS event_status;
    volatile unsigned char *progress;
};

#ifdef BUILD_SELECTED_START
static NTSTATUS __attribute__((ms_abi, noinline)) native_start(void *opaque)
{
    struct start_context *context = opaque;
    context->progress[16] = 'N'; /* Own output store, before any native C call. */
    context->entered = 1;
    context->progress[17] = 'P';
    context->native_call_ok = getpid() == context->process;
    context->progress[18] = 'p';
    context->progress[19] = 'T';
    void *initial = pthread_getspecific(context->key);
    context->progress[20] = 't';
    context->progress[21] = 'S';
    int set_status = pthread_setspecific(context->key, &context->marker);
    context->progress[22] = 's';
    context->progress[23] = 'G';
    void *actual = pthread_getspecific(context->key);
    context->progress[24] = 'g';
    context->tls_ok = initial == NULL && !set_status && actual == &context->marker;
    context->progress[25] = 'E';
    context->event_status = context->set_event(context->event, NULL);
    context->progress[26] = 'e';
    context->progress[27] = 'X';
    return context->native_call_ok && context->tls_ok && !context->event_status ? 0x3b21 : 0x3b22;
}
#endif

int RUN_TEST(start_fn selected_start)
{
    create_event_fn create_event = (create_event_fn)dlsym(RTLD_DEFAULT, "NtCreateEvent");
    create_thread_fn create_thread = (create_thread_fn)dlsym(RTLD_DEFAULT, "NtCreateThreadEx");
    set_event_fn set_event = (set_event_fn)dlsym(RTLD_DEFAULT, "NtSetEvent");
    close_fn close_handle = (close_fn)dlsym(RTLD_DEFAULT, "NtClose");
    resume_fn resume = (resume_fn)dlsym(RTLD_DEFAULT, "NtResumeThread");
    wait_fn wait = (wait_fn)dlsym(RTLD_DEFAULT, "NtWaitForSingleObject");
    terminate_fn terminate = (terminate_fn)dlsym(RTLD_DEFAULT, "NtTerminateThread");
    query_fn query = (query_fn)dlsym(RTLD_DEFAULT, "NtQueryInformationThread");
    struct start_context context = {.set_event = set_event, .process = getpid(), .marker = 0x61c0a5u};
    struct thread_basic_info info = {0};
    HANDLE thread = NULL;
    ULONG previous = 0, length = 0;
    int64_t timeout = -20000000; /* Each owned wait is bounded to two seconds. */
    bool key_created = false;
    int descriptor = -1;
    char path[PATH_MAX];
    const char *directory = getenv("CASKRUN_D3DMETAL_TEST_PROGRESS_DIR");
    int failure = 0;
#define CHECK(expression, code) do {if (!(expression)) {failure = code; goto cleanup;}} while (0)
    CHECK(directory && snprintf(path, sizeof(path), "%s/" CALLER_LABEL "-progress.bin", directory) < (int)sizeof(path), 15);
    CHECK((descriptor = open(path, O_CREAT | O_EXCL | O_RDWR, 0600)) >= 0, 16);
    CHECK(ftruncate(descriptor, 64) == 0, 17);
    context.progress = mmap(NULL, 64, PROT_READ | PROT_WRITE, MAP_SHARED, descriptor, 0);
    CHECK(context.progress != MAP_FAILED, 18);
    context.progress[0] = 'V';
    CHECK(selected_start && create_event && create_thread && set_event && close_handle && resume && wait && terminate && query, 1);
    CHECK(pthread_key_create(&context.key, NULL) == 0, 2);
    key_created = true;
    context.progress[1] = 'K';
    CHECK(create_event(&context.event, 0x1f0003, NULL, 0, 0) == 0 && context.event, 3);
    context.progress[2] = 'E';
    CHECK(create_thread(&thread, 0x1fffff, NULL, (HANDLE)(intptr_t)-1,
          selected_start, &context, 1, 0, 0x10000, 0x100000, NULL) == 0 && thread, 4);
    context.progress[3] = 'C';
    context.progress[4] = 'R';
    CHECK(resume(thread, &previous) == 0 && previous == 1, 5);
    context.progress[5] = 'r';
    CHECK(wait(context.event, 0, &timeout) == 0, 6);
    context.progress[6] = 'W';
    CHECK(wait(thread, 0, &timeout) == 0, 7);
    context.progress[7] = 'J';
    CHECK(context.entered == 1 && context.native_call_ok == 1 && context.tls_ok == 1 && !context.event_status, 8);
    CHECK(query(thread, 0, &info, sizeof(info), &length) == 0 && length == sizeof(info)
          && info.exit_status == 0x3b21, 9);
    context.progress[8] = 'Q';
    /* The callback's TLS value must belong to that thread, not the caller. */
    CHECK(pthread_getspecific(context.key) == NULL, 10);
cleanup:
    if (thread) {
        if (failure) {
            terminate(thread, 0);
            /* Never return past a live callback's stack context or delete its
             * TLS key. If the owned thread cannot be reaped, abort only this
             * fixture process; the supervisor still stops its owned prefix. */
            if (wait(thread, 0, &timeout) != 0) _exit(114);
        }
        if (close_handle(thread) != 0 && !failure) failure = 11;
    }
    if (context.event && close_handle(context.event) != 0 && !failure) failure = 12;
    if (key_created && pthread_key_delete(context.key) != 0 && !failure) failure = 13;
    if (context.progress && context.progress != MAP_FAILED) munmap((void *)context.progress, 64);
    if (descriptor >= 0) close(descriptor);
    return failure;
}
'''


def prepare():
    TEST.mkdir(exist_ok=True)
    host = module("d3dmetal_thread_build_idle", ROOT / "scripts/test-d3dmetal-host-dev.py")
    host.local_path(TEST, True)
    host.require_idle(TEST)
    (TEST / "thread-contract.h").write_text(SOURCE)
    (TEST / "sysv-caller.c").write_text('#define CALL_ABI\n#define CALLER_LABEL "sysv"\n#define RUN_TEST run_sysv_thread\n#include "thread-contract.h"\n')
    (TEST / "mock-region-support.c").write_text(r'''#define CALL_ABI __attribute__((ms_abi))
#define RUN_TEST run_ms_thread
#define CALLER_LABEL "msabi"
#define BUILD_SELECTED_START
#include "thread-contract.h"
bool supports_non_native_code_regions(void) {return true;}
static void report(const char *phase, int ms_status, int sysv_status) {
    const char *path = getenv("CASKRUN_D3DMETAL_TEST_RESULT");
    FILE *out = path ? fopen(path, "w") : NULL;
    if (out) {
        fprintf(out, "{\"phase\":\"%s\",\"passed\":%s,\"msabi_status\":%d,\"sysv_status\":%d}\n",
            phase, !ms_status && !sysv_status ? "true" : "false", ms_status, sysv_status);
        fclose(out);
    }
}
/* An own named callback, invoked from PE main through the reviewed Wine Unix
 * transition. Its four null/unused parameters are never treated as OS objects. */
int32_t __attribute__((ms_abi)) caskrun_owned_post_loader_thread_test(
    void *monitor, void *dc, void *rect, intptr_t data) {
    (void)monitor; (void)dc; (void)rect; (void)data;
    const char *path = getenv("CASKRUN_D3DMETAL_TEST_SYSV_CALLER");
    void *library = path ? dlopen(path, RTLD_NOW | RTLD_LOCAL) : NULL;
    int (*run_sysv)(start_fn) = library ? dlsym(library, "run_sysv_thread") : NULL;
    report("before_msabi_start", -1, -1);
    int ms_status = run_ms_thread(native_start);
    if (ms_status) {report("msabi_failed", ms_status, -1); return 0;}
    report("before_sysv_start", ms_status, -1);
    int sysv_status = run_sysv ? run_sysv(native_start) : 99;
    report("complete", ms_status, sysv_status);
    return !ms_status && !sysv_status;
}
void register_non_native_code_region(void *start, void *end) {
    static bool entered;
    (void)start; (void)end;
    if (entered) return;
    entered = true;
    const char *path = getenv("CASKRUN_D3DMETAL_TEST_ENTRY");
    FILE *out = path ? fopen(path, "wx") : NULL;
    if (out) {
        fprintf(out, "%llx\n", (unsigned long long)(uintptr_t)caskrun_owned_post_loader_thread_test);
        fclose(out);
    }
}
''')
    (TEST / "main.c").write_text(r'''#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdint.h>
#include <stdio.h>
typedef LONG (WINAPI *query_fn)(HANDLE, void *, ULONG, void *, SIZE_T, SIZE_T *);
typedef LONG (WINAPI *dispatch_fn)(UINT64, unsigned, void *);
/* Original macdrv callback entry2 transport; never an Apple/private layout. */
struct callback_params {UINT64 callback, monitor, dc, rect, data; DWORD result, last_error;};
_Static_assert(sizeof(struct callback_params) == 48, "owned callback transport");
static DWORD WINAPI ordinary_pe_start(void *event) {SetEvent(event); return 0x2721;}
static int identity_gate(const char *path) {
    char ready[32768], release[32768];
    HANDLE file;
    if (snprintf(ready, sizeof(ready), "%s.ready", path) >= (int)sizeof(ready) ||
        snprintf(release, sizeof(release), "%s.release", path) >= (int)sizeof(release)) return 0;
    if (GetFileAttributesA(release) != INVALID_FILE_ATTRIBUTES) return 0;
    file = CreateFileA(ready, GENERIC_WRITE, FILE_SHARE_READ, NULL, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) return 0;
    CloseHandle(file);
    while (GetFileAttributesA(release) == INVALID_FILE_ATTRIBUTES) Sleep(20);
    return 1;
}
static int pe_thread_control(void) {
    HANDLE event = CreateEventW(NULL, TRUE, FALSE, NULL), thread = NULL;
    DWORD status = 0;
    int failed = 1;
    if (!event) return 0;
    thread = CreateThread(NULL, 0, ordinary_pe_start, event, CREATE_SUSPENDED, NULL);
    if (!thread) goto done;
    if (ResumeThread(thread) != 1 || WaitForSingleObject(event, 2000) != WAIT_OBJECT_0 ||
        WaitForSingleObject(thread, 2000) != WAIT_OBJECT_0 || !GetExitCodeThread(thread, &status) || status != 0x2721) goto done;
    failed = 0;
done:
    if (thread) {
        if (failed) {TerminateThread(thread, 1); if (WaitForSingleObject(thread, 2000) != WAIT_OBJECT_0) ExitProcess(114);}
        CloseHandle(thread);
    }
    CloseHandle(event);
    return !failed;
}
int main(int argc, char **argv) {
    HMODULE ntdll = GetModuleHandleW(L"ntdll.dll"), driver;
    query_fn query = (query_fn)(void *)GetProcAddress(ntdll, "NtQueryVirtualMemory");
    dispatch_fn *dispatch = (dispatch_fn *)(void *)GetProcAddress(ntdll, "__wine_unix_call_dispatcher");
    UINT64 handle = 0;
    unsigned long long callback = 0;
    struct callback_params params = {0};
    HWND window;
    HDC dc;
    FILE *entry;
    int passed;
    if (argc != 3 || !query || !dispatch || !*dispatch) return 2;
    if (!pe_thread_control()) return 3;
    puts("{\"ordinary_pe_thread\":true}"); fflush(stdout);
    window = CreateWindowExW(0, L"STATIC", L"Caskrun owned native-thread fixture", WS_OVERLAPPEDWINDOW,
                             0, 0, 64, 64, NULL, NULL, GetModuleHandleW(NULL), NULL);
    if (!window) return 4;
    dc = GetDC(window);
    if (!dc) {DestroyWindow(window); return 4;}
    ReleaseDC(window, dc);
    driver = GetModuleHandleW(L"winemac.drv");
    entry = fopen(argv[1], "r");
    if (!entry) {DestroyWindow(window); return 5;}
    passed = fscanf(entry, "%llx", &callback) == 1 && callback;
    fclose(entry);
    /* MemoryWineUnixFuncs=1000 is Wine11's published Unix-call query. */
    if (!passed || !driver || query(GetCurrentProcess(), driver, 1000, &handle, sizeof(handle), NULL) || !handle) {
        DestroyWindow(window); return 6;
    }
    if (!identity_gate(argv[2])) {DestroyWindow(window); return 8;}
    params.callback = callback;
    passed = (*dispatch)(handle, 2, &params) == 0 && params.result == 1;
    if (!DestroyWindow(window)) passed = 0;
    printf("{\"fixture_main_reached\":%s}\n", passed ? "true" : "false");
    return passed ? 0 : 7;
}
''')
    flags = ["clang", "-arch", "x86_64", "-std=gnu11", "-O2", "-Wall", "-Wextra", "-Werror",
             "-fno-optimize-sibling-calls", "-dynamiclib"]
    for source, output in (("sysv-caller.c", "libsysv-caller.dylib"),
                           ("mock-region-support.c", "libmock-region-support.dylib")):
        subprocess.run(flags + [source, "-o", output], cwd=TEST, check=True, timeout=30)
    subprocess.run(["x86_64-w64-mingw32-gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                    "main.c", "-o", "native-thread.exe", "-luser32", "-lgdi32"], cwd=TEST, check=True, timeout=30)


def run():
    host = module("d3dmetal_thread_idle", ROOT / "scripts/test-d3dmetal-host-dev.py")
    runner = host.runner
    package = host.local_path(WORK / "private-runtime", True)
    host.require_idle(package)
    proof = host.harness.read_json(host.local_path(WORK / "combined-candidate-proof.json"))
    for relative, expected in host.EXECUTABLES.items():
        runner.require(host.harness.digest(host.local_path(package / relative)) == expected,
                       "A reviewed execution input changed")
    artifacts = {name: host.local_path(package / relative) for name, relative in host.HOST_FILES.items()}
    for name, path in artifacts.items():
        runner.require(host.harness.digest(path) == proof["host_files"][name], "A reviewed host image changed")
    host.checked_source_patches(proof, require_thread_validation=False)
    artifacts["libmock-region-support.dylib"] = host.local_path(TEST / "libmock-region-support.dylib")
    expected = {name: host.harness.digest(path) for name, path in artifacts.items()}
    previous = TEST / "result.json"
    if previous.exists() and not (TEST / "prior-start-only-result.json").exists():
        shutil.copyfile(previous, TEST / "prior-start-only-result.json")
    runner.EXPERIMENT = TEST
    session = runner.Session(package)
    result = {"passed": False, "mock_support_only": True, "native_start_callback": True}
    try:
        wine, env = package / "bin/wine", session.environment("wined3d")
        code, _ = session.invoke([wine, "wineboot.exe", "-u"], env, "bootstrap", 60)
        runner.require(code == 0, "Private bootstrap failed")
        for key, value in (("Auto", "1"), ("Debugger", "false.exe")):
            code, _ = session.invoke([wine, "reg.exe", "add", "HKLM\\Software\\Microsoft\\Windows NT\\CurrentVersion\\AeDebug",
                "/v", key, "/t", "REG_SZ", "/d", value, "/f"], env, "debugger-" + key, 10)
            runner.require(code == 0, "Could not suppress fixture crash UI")
        session.stop_and_check_idle()
        fixture = session.root / "app/native-thread.exe"
        shutil.copyfile(TEST / "native-thread.exe", fixture)
        output = session.root / "native-thread.json"
        entry = session.root / "own-post-loader-entry.txt"
        gate = session.root / "identity"
        env.update(CASKRUN_D3DMETAL_SHARED_LIBRARY=str(TEST / "libmock-region-support.dylib"),
                   CASKRUN_D3DMETAL_TEST_SYSV_CALLER=str(TEST / "libsysv-caller.dylib"),
                   CASKRUN_D3DMETAL_TEST_RESULT=str(output),
                   CASKRUN_D3DMETAL_TEST_ENTRY=str(entry),
                   CASKRUN_D3DMETAL_TEST_PROGRESS_DIR=str(session.root))

        def inspect():
            runner.contract.verify_live_mappings(session, fixture, artifacts, expected)
            session.manifest["tracked"].update(session._mapped_probe_processes)
            result["actual_host_mapping_verified"] = True

        code, data = session.invoke([wine, fixture, "Z:" + str(entry).replace("/", "\\"),
            "Z:" + str(gate).replace("/", "\\")], env, "native-thread", 30, gate, inspect)
        runner.require(output.is_file(), "Native thread fixture produced no result")
        result.update(json.loads(output.read_text()))
        result.update(exit_code=code, fixture_main_reached=b'"fixture_main_reached":true' in data)
        result["ordinary_pe_thread_passed"] = b'"ordinary_pe_thread":true' in data
        result["passed"] = (result["passed"] and code == 0 and result["phase"] == "complete"
                            and result["fixture_main_reached"] and result["ordinary_pe_thread_passed"]
                            and result.get("actual_host_mapping_verified") is True)
        for name, path in artifacts.items():
            runner.require(host.harness.digest(path) == expected[name], "A host image changed during the fixture")
    except BaseException as error:
        result.update(passed=False, error=str(error))
    finally:
        try:
            session.stop_and_check_idle()
            session._validate_owner()
            labels = {0: "output_initialized", 1: "key_created", 2: "event_created",
                3: "thread_created_suspended", 4: "before_resume", 5: "resume_returned",
                6: "event_wait_completed", 7: "thread_wait_completed", 8: "exit_status_verified",
                16: "native_callback_entered", 17: "before_getpid", 18: "after_getpid",
                19: "before_pthread_getspecific_initial", 20: "after_pthread_getspecific_initial",
                21: "before_pthread_setspecific", 22: "after_pthread_setspecific",
                23: "before_pthread_getspecific_verify", 24: "after_pthread_getspecific_verify",
                25: "before_owned_set_event", 26: "after_owned_set_event", 27: "before_callback_return"}
            result["progress"] = {}
            for caller in ("msabi", "sysv"):
                path = session.root / (caller + "-progress.bin")
                if path.exists():
                    host.local_path(path)
                    data = path.read_bytes()
                    runner.require(len(data) == 64, "Own progress output has an unexpected size")
                    result["progress"][caller] = [label for offset, label in labels.items() if data[offset]]
                    shutil.copyfile(path, TEST / path.name)
            for name in ("native-thread.jsonl", "native-thread.stderr"):
                path = session.raw / name
                if path.exists():
                    host.local_path(path)
                    runner.require(path.stat().st_size <= runner.MAX_LOG, "Fixture proof exceeded its bounded limit")
                    shutil.copyfile(path, TEST / name)
            runner.require(shutil.rmtree.avoids_symlink_attacks, "Safe fixture removal unavailable")
            shutil.rmtree(session.root)
            result["cleanup"] = "removed_after_no_owners_verified"
        except BaseException as error:
            result.update(passed=False, cleanup_error=str(error))
        result["passed"] = result["passed"] and result.get("cleanup") == "removed_after_no_owners_verified"
        (TEST / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        print(json.dumps({name: value for name, value in result.items() if name not in ("error", "cleanup_error")}))
    return 0 if result["passed"] else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", action="store_true", help="run after preparing; uses an owned disposable Wine prefix")
    args = parser.parse_args()
    prepare()
    if args.run:
        return run()
    print(json.dumps({"prepared": True, "wine_executed": False, "apple_code_executed": False}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
