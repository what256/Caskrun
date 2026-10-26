#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Prepare, or explicitly run, the owned no-Apple Wine registry/mock fixture."""
import argparse
import importlib.util
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
PROJECT = HERE.parents[2]
RUNTIME = PROJECT / '.build/native-text-wine-runtime-dev-v16'
BUILD = PROJECT / '.build'
STAGE = 'prepare'


def stage(label):
    global STAGE
    STAGE = label
PINS = {
    'runtime/bin/winebuild': '502e911b25c4463d380fdf5dc13fc1e99d0828ba5b116d47e0c892d789179130',
    'runtime/bin/wineserver': '5427fad31731e8f427e93756c47712745deb3e677a8f7971f0da0a9709eeab66',
    'runtime/lib/wine/x86_64-windows/ntdll.dll': '09e72e70393ad42ce996d73f87f39c6334c3351e3c4cb05726daf75447e4b3e3',
    'runtime/lib/wine/x86_64-unix/ntdll.so': 'fa562ffa6f0d5d4fdc5a6587ba7d0b27313dbf8e95fca5e5a1a8f354cd961070',
    'bin/_dispatch': '48fa40c71c6d9332573c378e402d94b8de0556d56b973339fe52f63c4a304b3a',
}


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def command(args, timeout=90):
    subprocess.run([str(x) for x in args], stdin=subprocess.DEVNULL,
                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True, timeout=timeout)


def no_open_owners(directory):
    check = subprocess.run(['/usr/sbin/lsof', '-nP', '-Fpc', '+D', str(directory)],
                           stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                           timeout=30, check=False)
    if check.returncode != 1 or check.stdout or check.stderr:
        raise RuntimeError('Owned scratch still has owners or cannot be checked; preserved')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run', action='store_true', help='Run in a disposable Wine prefix')
    args = parser.parse_args()
    runner = load('caskrun_mock_session', PROJECT / 'scripts/test-dxmt-d3d12-dev.py')
    marker = load('caskrun_mock_mark', PROJECT / 'scripts/package-wine-engine-components-dev.py')
    if marker.P.tree_identity(RUNTIME) != 'a235fd32318839ec2aa164befec58a83bad8bd988ec04f7433e9a8faa936d9fa':
        raise RuntimeError('The complete V16 runtime does not match its reviewed identity')
    for relative, digest in PINS.items():
        path = runner.harness.real_path(RUNTIME / relative)
        if runner.harness.digest(path) != digest:
            raise RuntimeError('A reviewed Wine input does not match its pin')
    for name in ('clang++', 'x86_64-w64-mingw32-gcc'):
        if not shutil.which(name):
            raise RuntimeError('A required compiler is unavailable')
    if not args.run:
        print(json.dumps({'prepared': True, 'executed': False, 'apple_loaded': False}))
        return

    runner.harness.real_path(BUILD, directory=True)
    scratch = Path(tempfile.mkdtemp(prefix='gfxt-wine-mock-', dir=BUILD))
    identity = scratch.stat()
    runtime = scratch / 'runtime-clone'
    session = None
    try:
        stage('compile-owned-fixture')
        command(['clang++', '-arch', 'x86_64', '-std=c++17', '-O2', '-Wall', '-Wextra', '-Werror',
                 '-dynamiclib', HERE/'native.cpp', HERE/'mock.cpp', '-o', scratch/'probe.so'])
        command(['x86_64-w64-mingw32-gcc', '-std=c11', '-O2', '-Wall', '-Wextra', '-Werror',
                 '-shared', HERE/'pe.c', '-o', scratch/'caskrun_gfxt_probe.dll'])
        command(['x86_64-w64-mingw32-gcc', '-std=c11', '-O2', '-Wall', '-Wextra', '-Werror',
                 HERE/'fixture.c', '-ladvapi32', '-o', scratch/'fixture.exe'])
        stage('clone-owned-runtime')
        runtime.mkdir(mode=0o700)
        for name in ('bin', 'lib', 'runtime'):
            command(['/bin/cp', '-cR', RUNTIME/name, runtime/name], timeout=180)
        pe = runtime/'runtime/lib/wine/x86_64-windows/caskrun_gfxt_probe.dll'
        native = runtime/'runtime/lib/wine/x86_64-unix/caskrun_gfxt_probe.so'
        shutil.copyfile(scratch/'caskrun_gfxt_probe.dll', pe)
        marker.mark_wine_builtin(pe, RUNTIME/'runtime/bin/winebuild', 0x8664)
        shutil.copyfile(scratch/'probe.so', native)
        command(['/usr/bin/codesign', '--force', '--sign', '-', native])
        command(['/usr/bin/codesign', '--verify', '--strict', native])
        stage('initialize-owned-prefix')
        runner.EXPERIMENT = scratch
        session = runner.Session(runtime)
        env = session.environment('wined3d')
        env['WINEDLLOVERRIDES'] = 'caskrun_gfxt_probe=b;mscoree,mshtml='
        status, _ = session.invoke([runtime/'bin/wine', 'wineboot.exe', '-u'], env, 'wineboot', 60)
        if status:
            raise RuntimeError('Disposable prefix initialization failed')
        session.stop_and_check_idle()
        gate = session.root/'identity'
        fixture = scratch/'fixture.exe'
        artifacts = {'caskrun_gfxt_probe.dll': pe, 'caskrun_gfxt_probe.so': native,
                     'ntdll.dll': runtime/'runtime/lib/wine/x86_64-windows/ntdll.dll',
                     'ntdll.so': runtime/'runtime/lib/wine/x86_64-unix/ntdll.so'}
        hashes = {name: runner.harness.digest(path) for name, path in artifacts.items()}

        def inspect():
            runner.contract.verify_live_mappings(session, fixture, artifacts, hashes)
            for pid, birth in session._mapped_probe_processes.items():
                session.manifest['tracked'][pid] = birth
                result = subprocess.run(['/usr/sbin/lsof', '-nP', '-a', '-p', pid, '-d', 'txt', '-F0pfn'],
                                        stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5, check=False)
                if result.returncode or result.stderr:
                    raise RuntimeError('Owned fixture mapping inspection failed')
                paths = runner.contract.mapped_text_paths(result.stdout, int(pid))
                if any('D3DMetal.framework' in str(p) or p.name == 'libd3dshared.dylib' for p in paths):
                    raise RuntimeError('Unexpected Apple component mapped')

        windows_gate = 'Z:' + str(gate).replace('/', '\\')
        stage('mock-and-mapping-check')
        try:
            status, output = session.invoke([runtime/'bin/wine', fixture, windows_gate], env, 'probe', 40, gate, inspect)
        except runner.Error:
            diagnostics = session.raw / 'probe.stderr'
            allowed = {'registry-create', 'registry-write', 'dll-load', 'dll-exports',
                       'pe-prepare', 'identity-gate', 'mock-call', 'registry-read', 'cleanup'}
            if diagnostics.exists() and diagnostics.stat().st_size <= 65536:
                labels = re.findall(rb'(?m)^CASKRUN_MOCK_STAGE:([a-z-]+)\r?$', diagnostics.read_bytes())
                if len(labels) == 1 and labels[0].decode('ascii') in allowed:
                    raise RuntimeError('Owned fixture stopped at ' + labels[0].decode('ascii')) from None
            raise
        expected = {'fixture_passed': True, 'actual_wine_registry': True, 'checks': 7,
                    'native_pointers_returned': 0, 'apple_loaded': False, 'game_compatibility_verified': False}
        if status or json.loads(output.decode().strip()) != expected:
            raise RuntimeError('Wine mock fixture did not pass')
    finally:
        try:
            if session:
                session.stop_and_check_idle()
                session._validate_owner()
            current = scratch.stat()
            if current.st_uid != os.getuid() or (current.st_dev, current.st_ino) != (identity.st_dev, identity.st_ino):
                raise RuntimeError('Owned scratch identity changed; preserved')
            no_open_owners(scratch)
            if not shutil.rmtree.avoids_symlink_attacks:
                raise RuntimeError('Safe recursive deletion unavailable; owned scratch preserved')
            shutil.rmtree(scratch)
        except Exception:
            stage('cleanup')
            raise
    print(json.dumps({'fixture_passed': True, 'checks': 7, 'native_pointers_returned': 0,
                      'apple_absent_at_mapping_gate': True, 'owned_scratch_removed': True}))


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        # Only our fixed validation messages may be shown. Never print command arguments or environment.
        detail = str(error) if type(error).__name__ in ('HarnessError', 'RuntimeError') else ''
        detail = detail.replace(str(Path.home()), '~').replace('\n', ' ')[:300]
        print('Mock probe failed at ' + STAGE + ': ' + type(error).__name__ +
              (': ' + detail if detail else ''), file=sys.stderr)
        sys.exit(1)
