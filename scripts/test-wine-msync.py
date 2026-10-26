#!/usr/bin/env python3
"""Bounded x86/x64 MSync regression checks in new, isolated Wine prefixes.

The supplied runtime must already contain a matching reviewed ntdll and wineserver.
No existing bottle or globally running Wine server is touched. Full logs stay in
ignored .build. All test prefixes are stopped and removed before this returns.
"""
from __future__ import annotations
import argparse
import concurrent.futures
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

REPO = Path(__file__).resolve().parents[1]
PROBE = REPO / "scripts/probes/msync-sync.c"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", required=True, type=Path)
    parser.add_argument("--backend", choices=("wined3d", "dxmt"), default="wined3d")
    args = parser.parse_args()
    build_root = (REPO / ".build").resolve()
    runtime = args.runtime.resolve(strict=True)
    if not runtime.is_relative_to(build_root) or runtime == build_root:
        parser.error("runtime must be an owned candidate below this project's .build")
    if runtime.stat().st_uid != os.getuid() or not runtime.is_dir():
        parser.error("runtime candidate must be a directory owned by this user")
    wine, server = runtime / "bin/wine", runtime / "bin/wineserver"
    for executable in (wine, server):
        resolved = executable.resolve(strict=True)
        if (not resolved.is_relative_to(runtime) or not resolved.is_file()
                or resolved.stat().st_uid != os.getuid()
                or not os.access(resolved, os.X_OK)
                or "/crossover.app/" in str(resolved).lower()
                or "/crossover-lab/" in str(resolved).lower()):
            parser.error("runtime executable must stay inside the owned candidate")
    build_root.mkdir(exist_ok=True)
    session = Path(tempfile.mkdtemp(prefix="msync-regression-", dir=build_root))
    home = session / "home"
    home.mkdir()
    prefixes = [session / f"prefix-{mode}" for mode in (0, 1)]
    failed = False

    def environment(mode: int) -> dict[str, str]:
        env = dict(os.environ)
        for key in list(env):
            if key.startswith(("WINE", "CASKRUN_", "DYLD_")):
                del env[key]
        env.update(HOME=str(home), WINEPREFIX=str(prefixes[mode]), WINEMSYNC=str(mode),
                   WINEDEBUG="-all", WINESERVER=str(server), WINELOADER=str(wine),
                   CASKRUN_GRAPHICS_BACKEND=args.backend,
                   WINEDLLOVERRIDES="winemenubuilder.exe=d", DYLD_LIBRARY_PATH=str(runtime / "lib"),
                   PATH=str(runtime / "bin") + ":/usr/bin:/bin:/usr/sbin:/sbin")
        return env

    def checked(command: list[str], log: Path, env: dict[str, str] | None, seconds: int) -> str:
        with log.open("wb") as stream:
            result = subprocess.run(command, env=env, stdout=stream, stderr=subprocess.STDOUT,
                                    timeout=seconds, check=False)
        if result.returncode:
            raise RuntimeError(f"{log.name} failed with status {result.returncode}")
        return log.read_text(errors="replace")

    def run_mode(mode: int) -> list[str]:
        results = []
        env = environment(mode)
        checked([str(wine), "wineboot", "-u"], session / f"bootstrap-{mode}.log", env, 60)
        for arch in ("x86_64", "i686"):
            text = checked([str(wine), str(session / f"sync-{arch}.exe")],
                           session / f"mode{mode}-{arch}.log", env, 35)
            summary = next((line.strip() for line in text.splitlines() if line.startswith("RESULT ")), None)
            if not summary or "failures=0" not in summary:
                raise RuntimeError(f"mode{mode}-{arch} has no successful result")
            if mode == 1 and arch == "x86_64":
                startup = (session / f"bootstrap-{mode}.log").read_text(errors="replace")
                if "msync: up and running." not in startup + text:
                    raise RuntimeError("mode1 did not confirm the Mach synchronization server")
            results.append(f"{args.backend} WINEMSYNC={mode} {arch}: {summary}")
        return results

    try:
        for arch in ("x86_64", "i686"):
            compiler = shutil.which(arch + "-w64-mingw32-gcc")
            if not compiler:
                raise RuntimeError("Missing MinGW compiler: " + arch)
            checked([compiler, "-O2", "-Wall", "-Wextra", "-Werror", str(PROBE), "-o", str(session / f"sync-{arch}.exe")],
                    session / f"compile-{arch}.log", None, 30)
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            futures = [pool.submit(run_mode, mode) for mode in (0, 1)]
            for future in futures:
                for summary in future.result():
                    print(summary, flush=True)
    except (OSError, RuntimeError, subprocess.TimeoutExpired) as exc:
        print(str(exc), file=sys.stderr)
        failed = True
    finally:
        for mode, prefix in enumerate(prefixes):
            if prefix.exists():
                try:
                    subprocess.run([str(server), "-k"], env=environment(mode), timeout=10,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
                    stopped = subprocess.run([str(server), "-w"], env=environment(mode), timeout=15,
                                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
                    if stopped.returncode:
                        raise RuntimeError("The owned prefix server stop could not be verified")
                    owners = subprocess.run(["/usr/sbin/lsof", "-t", "+D", str(prefix)],
                                            capture_output=True, timeout=20, check=False)
                    if owners.returncode not in (0, 1):
                        raise RuntimeError("The owned prefix lsof check failed")
                    if owners.stdout.strip():
                        failed = True
                        print("Test prefix still has owners; left in ignored .build.", file=sys.stderr)
                    else:
                        shutil.rmtree(prefix)
                except (OSError, RuntimeError, subprocess.TimeoutExpired):
                    failed = True
                    print("Test-prefix cleanup could not be verified; retained in .build.", file=sys.stderr)
        # Retain diagnostics and compiled probes until reviewed; no bottles survive a verified stop.
    print("MSync regression " + ("FAILED" if failed else "PASSED"))
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
