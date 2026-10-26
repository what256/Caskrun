#!/usr/bin/env python3
"""Check the original source-built NT import ABI thunks without Apple binaries.

This is an argument/return preservation test, not an NT implementation or a
D3DMetal compatibility claim. Generated native fixtures stay under .build/.
"""

from __future__ import annotations

import json
from pathlib import Path
import re
import subprocess


ROOT = Path(__file__).resolve().parents[1]
WORK = ROOT / ".build/d3dmetal-bridge-integration/abi-contract-test"
PATCH = ROOT / "runtime-patches/d3dmetal-wine-11.0-native-import-abi.patch"

CONTRACTS = [
    ("NtCreateSemaphore", "HANDLE *handle, ACCESS_MASK access, const OBJECT_ATTRIBUTES *attr, LONG initial, LONG max"),
    ("NtReleaseSemaphore", "HANDLE handle, ULONG count, ULONG *previous"),
    ("NtCreateEvent", "HANDLE *handle, ACCESS_MASK access, const OBJECT_ATTRIBUTES *attr, EVENT_TYPE type, BOOLEAN state"),
    ("NtSetEvent", "HANDLE handle, LONG *prev_state"),
    ("NtResetEvent", "HANDLE handle, LONG *prev_state"),
    ("NtClearEvent", "HANDLE handle"),
    ("NtPulseEvent", "HANDLE handle, LONG *prev_state"),
    ("NtCreateThreadEx", "HANDLE *handle, ACCESS_MASK access, OBJECT_ATTRIBUTES *attr, HANDLE process, PRTL_THREAD_START_ROUTINE start, void *param, ULONG flags, ULONG_PTR zero_bits, SIZE_T stack_commit, SIZE_T stack_reserve, PS_ATTRIBUTE_LIST *attr_list"),
    ("NtClose", "HANDLE handle"),
]


def run(*args: str) -> str:
    result = subprocess.run(args, cwd=WORK, text=True, capture_output=True, timeout=30)
    if result.returncode:
        raise RuntimeError(f"{Path(args[0]).name} failed: {result.stderr[-4000:]}")
    return result.stdout


def main() -> None:
    WORK.mkdir(parents=True, exist_ok=True)
    additions = "\n".join(line[1:] for line in PATCH.read_text().splitlines()
                          if line.startswith("+") and not line.startswith("+++"))
    macro_start = additions.index("#define CASKRUN_D3DMETAL_ABI_EXPORT")
    macro_end = additions.index("\n#endif", macro_start)
    macro = additions[macro_start:macro_end]
    assert "(%rsp), %r10" in macro and '"1:\\n\\tjmp "' in macro
    header = """#include <stdint.h>
#include <stddef.h>
typedef int32_t NTSTATUS, LONG;
typedef uint32_t ULONG, ACCESS_MASK, EVENT_TYPE;
typedef uint8_t BOOLEAN;
typedef uintptr_t ULONG_PTR, SIZE_T;
typedef void *HANDLE;
typedef struct object_attributes OBJECT_ATTRIBUTES;
typedef struct attribute_list PS_ATTRIBUTE_LIST;
typedef NTSTATUS (*PRTL_THREAD_START_ROUTINE)(void *);
#define __ASM_NAME(name) "_" name
extern uintptr_t caskrun_d3dmetal_text_begin, caskrun_d3dmetal_text_end;
"""
    implementations = [header, macro, "uintptr_t caskrun_d3dmetal_text_begin, caskrun_d3dmetal_text_end;",
                       "unsigned calls[9], bad_arguments;"]
    declarations = []
    invocations = []
    for number, (name, signature) in enumerate(CONTRACTS):
        args = [arg.strip() for arg in signature.split(",")]
        names = [re.search(r"([A-Za-z_][A-Za-z0-9_]*)$", arg).group(1) for arg in args]
        types = [arg[:arg.rfind(arg_name)].strip() for arg, arg_name in zip(args, names)]
        values = [f"({kind})(uintptr_t)0x{(number + 1) * 0x1000 + index * 0x10 + 7:x}"
                  for index, kind in enumerate(types)]
        checks = " || ".join(f"{arg} != {value}" for arg, value in zip(names, values))
        implementations.append(f"NTSTATUS caskrun_sysv_{name}({signature}) {{ "
                               f"calls[{number}]++; if ({checks}) bad_arguments++; "
                               f"return 0x12340000 + {number}; }}")
        pattern = (r"NTSTATUS __attribute__\(\(ms_abi\)\) caskrun_ms_" + name +
                   r"\(\s*(.*?)\s*\)\n\{\n    return (.*?);\n\}")
        match = re.search(pattern, additions)
        assert match and match.group(1) == signature, f"signature drift: {name}"
        implementations.append(match.group(0))
        implementations.append(f"CASKRUN_D3DMETAL_ABI_EXPORT({name}, caskrun_sysv_{name}, caskrun_ms_{name})")
        declarations.append(f"NTSTATUS CALL_ABI {name}({signature});")
        invocations.append(f"if ({name}({', '.join(values)}) != 0x12340000 + {number}) return {number + 1};")
    body = "\n".join(invocations)
    (WORK / "contract.c").write_text("\n".join(implementations))
    (WORK / "contract.h").write_text(header + "\n" + "\n".join(declarations))
    (WORK / "caller.c").write_text('#define CALL_ABI __attribute__((ms_abi))\n#include "contract.h"\n'
                                  f"int run_ms_calls(void) {{ {body} return 0; }}\n")
    (WORK / "main.c").write_text('''#define CALL_ABI
#include "contract.h"
#include <dlfcn.h>
#include <mach-o/getsect.h>
#include <stdio.h>
extern unsigned calls[9], bad_arguments;
static int selected(uintptr_t address) {
    return caskrun_d3dmetal_text_begin && address >= caskrun_d3dmetal_text_begin &&
           address < caskrun_d3dmetal_text_end;
}
int main(void) {
    void *module = dlopen("./libcaller.dylib", RTLD_NOW | RTLD_LOCAL);
    int (*run_ms_calls)(void);
    Dl_info info;
    unsigned long size = 0;
    const uint8_t *text;
    if (!module || !(run_ms_calls = dlsym(module, "run_ms_calls"))) return 20;
    if (!dladdr((void *)run_ms_calls, &info) || !info.dli_fbase ||
        !(text = getsegmentdata(info.dli_fbase, "__TEXT", &size)) || !size ||
        (uintptr_t)text > UINTPTR_MAX - size) return 21;
    /* First exercise the default all-SysV path. */
''' + body + '''
    caskrun_d3dmetal_text_begin = (uintptr_t)text;
    caskrun_d3dmetal_text_end = (uintptr_t)text + size;
    if (selected(0) || selected(UINTPTR_MAX) || selected((uintptr_t)text - 1) ||
        !selected((uintptr_t)text) || !selected((uintptr_t)text + size - 1) ||
        selected((uintptr_t)text + size)) return 25;
    if (run_ms_calls()) return 22;
    /* Range enabled: unrelated SysV callers must still work. */
''' + body + '''
    if (bad_arguments) return 23;
    for (unsigned i = 0; i < 9; i++) if (calls[i] != 3) return 24;
    caskrun_d3dmetal_text_begin = caskrun_d3dmetal_text_end = 0;
    if (selected((uintptr_t)text) || selected(0)) return 26;
    puts("{\\"passed\\":true,\\"signatures\\":9,\\"calls\\":27,\\"msabi_calls\\":9,\\"sysv_calls\\":18,\\"argument_mismatches\\":0,\\"range_boundary_checks\\":8}");
    return 0;
}
''')
    flags = ["clang", "-arch", "x86_64", "-std=gnu11", "-O2", "-Wall", "-Wextra", "-Werror",
             "-fno-optimize-sibling-calls"]
    run(*flags, "-dynamiclib", "contract.c", "-o", "libcontract.dylib",
        "-Wl,-install_name,@rpath/libcontract.dylib")
    run(*flags, "-dynamiclib", "caller.c", "-o", "libcaller.dylib", "-L.", "-lcontract",
        "-Wl,-rpath,@loader_path")
    run(*flags, "main.c", "-o", "abi-test", "-L.", "-lcontract", "-Wl,-rpath,@loader_path")
    result = json.loads(run("./abi-test"))
    (WORK / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
