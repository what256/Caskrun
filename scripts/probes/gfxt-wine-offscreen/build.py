#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Build-only, owned offscreen fixture; never load Apple or launch Wine."""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import sys
import tarfile
import tempfile

HERE = Path(__file__).resolve().parent
PROJECT = HERE.parents[2]
BUILD = PROJECT / '.build'
UTM_SHA = '72e9e3552e508463ce3c035de76a767671184027b68668c505e86ed2fffca88b'
DX_SHA = 'da4517d5c31465a5ec03d4c679371da178ef010cb38655dac5e1d7e71d201630'
PATCH_SHA = '506f3a5a3c8581ce189e11d36f52d84943cd0f02a054e18515b364bc2c1cc38e'
HOST_SOURCES = {'dmn_init.cpp', 'dmn_log.cpp', 'dmn_gfxt_os.cpp', 'dmn_gfxt_event.cpp',
                'dmn_gfxt_path.cpp', 'dmn_gfxt_registry.cpp', 'dmn_gfxt_allocation.cpp',
                'dmn_gfxt_library.cpp', 'dmn_gfxt_monitor.cpp', 'dmn_gfxt_adapter.mm',
                'dmn_window.mm', 'dmn_gfxt_swapchain.mm'}
DIRECTX_SOURCES = {'dxn_version.c', 'dxn_log.c', 'dxn_shm.c', 'dxn_shm_darwin.c', 'dxn_event_kqueue.c'}


def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(block)
    return value.hexdigest()


def extract(archive, destination, expected_root):
    destination.mkdir(mode=0o700)
    seen = set()
    total = 0
    with tarfile.open(archive, 'r:gz') as tar:
        for entry in tar:
            path = PurePosixPath(entry.name)
            if (path.is_absolute() or not path.parts or path.parts[0] != expected_root
                    or any(part in ('.', '..') for part in path.parts)
                    or not (entry.isfile() or entry.isdir())):
                raise RuntimeError('Archive has an unsafe entry')
            parts = path.parts[1:]
            if not parts:
                if not entry.isdir():
                    raise RuntimeError('Invalid archive root')
                continue
            relative = Path(*parts)
            if relative in seen:
                raise RuntimeError('Archive contains duplicate paths')
            seen.add(relative)
            target = destination / relative
            if entry.isdir():
                target.mkdir(mode=0o700, parents=True, exist_ok=True)
            else:
                total += entry.size
                if entry.size < 0 or total > 256 * 1024 * 1024:
                    raise RuntimeError('Archive is too large')
                target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
                with tar.extractfile(entry) as source, target.open('xb') as out:
                    shutil.copyfileobj(source, out)


def run(args, cwd, env):
    return subprocess.run([str(arg) for arg in args], cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True, timeout=240).stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--build', action='store_true', help='Compile into a fresh owned scratch directory')
    parser.add_argument('--utm-archive', type=Path, required=True)
    parser.add_argument('--directx-archive', type=Path, required=True)
    parser.add_argument('--meson', type=Path, help='Path to a local Meson executable')
    parser.add_argument('--ninja', type=Path, help='Path to a local Ninja executable')
    args = parser.parse_args()
    for file, wanted in ((args.utm_archive, UTM_SHA), (args.directx_archive, DX_SHA),
                         (HERE/'basic-host-only.patch', PATCH_SHA)):
        if file.is_symlink() or not file.is_file() or digest(file) != wanted:
            raise RuntimeError('A source archive or patch does not match its pinned hash')
    tools = {name: shutil.which(name) for name in ('meson', 'ninja', 'clang++',
             'x86_64-w64-mingw32-gcc', 'patch')}
    for name in ('meson', 'ninja'):
        supplied = getattr(args, name)
        if supplied:
            resolved = supplied.resolve(strict=True)
            if not resolved.is_file() or not os.access(resolved, os.X_OK):
                raise RuntimeError('A selected build tool is not executable')
            tools[name] = str(resolved)
    if not all(tools.values()):
        raise RuntimeError('A required build tool is unavailable')
    if not args.build:
        print(json.dumps({'prepared': True, 'built': False, 'apple_executed': False}))
        return
    if not BUILD.is_dir() or BUILD.is_symlink():
        raise RuntimeError('Project build directory must be a real directory')
    scratch = Path(tempfile.mkdtemp(prefix='gfxt-offscreen-build-', dir=BUILD))
    source = scratch/'source'
    out = scratch/'artifacts'
    out.mkdir(mode=0o700)
    tool_dirs = [str(Path(value).resolve().parent) for value in tools.values()]
    env = {'PATH': os.pathsep.join(tool_dirs + ['/usr/bin', '/bin', '/usr/sbin', '/sbin', '/opt/homebrew/bin']),
           'TMPDIR': str(scratch), 'XDG_CACHE_HOME': str(scratch/'cache'),
           'XDG_CONFIG_HOME': str(scratch/'config'), 'LANG': 'C'}
    if 'HOME' in os.environ:
        env['HOME'] = os.environ['HOME']
    try:
        extract(args.utm_archive, source, 'd3dmetal-native-422df8736de0f371d5ecee71e04b3ed19c4a2183')
        extract(args.directx_archive, source/'subprojects/directx-native',
                'directx-native-7258be1c56cdbf1e55c56b1bc4a866169c8ca0da')
        run([tools['patch'], '-p1', '-i', HERE/'basic-host-only.patch'], source, env)
        build = scratch/'host-build'
        run([tools['meson'], 'setup', build, source, '--cross-file', source/'build-macos-x86_64.txt',
             '--buildtype=release', '--wrap-mode=forcefallback', '-Dtests=disabled',
             '-Ddev_framework_path=/CASKRUN_FIXTURE_NO_FRAMEWORK'], scratch, env)
        run([tools['ninja'], '-C', build, 'libd3dmetal-native.dylib'], scratch, env)
        commands = json.loads((build/'compile_commands.json').read_text())
        inventory = {}
        for item in commands:
            path = Path(item['file'])
            if not path.is_absolute():
                path = (Path(item['directory']) / path).resolve()
            relative = path.relative_to(source)
            if relative.parts[0] == 'src' and len(relative.parts) == 2 and relative.name in HOST_SOURCES:
                group = 'host'
            elif (relative.parts[:3] == ('subprojects', 'directx-native', 'src')
                  and len(relative.parts) == 4 and relative.name in DIRECTX_SOURCES):
                group = 'directx'
            else:
                raise RuntimeError('Unexpected host compilation source')
            if relative.name in inventory:
                raise RuntimeError('Duplicate host compilation source')
            obj = Path(item['output'])
            if obj.is_absolute() or '..' in obj.parts:
                raise RuntimeError('Unexpected host object path')
            built_object = build/obj
            if built_object.is_symlink() or not built_object.is_file():
                raise RuntimeError('Expected host object is missing')
            inventory[relative.name] = {'group': group, 'object': str(obj), 'sha256': digest(built_object)}
        if set(inventory) != HOST_SOURCES | DIRECTX_SOURCES:
            raise RuntimeError('Host compilation inventory differs from allowlist')
        for group, target in (('host', 'libd3dmetal-native.dylib'),
                              ('directx', 'subprojects/directx-native/libdirectx-native.a')):
            query = run([tools['ninja'], '-C', build, '-t', 'query', target], scratch, env).decode()
            required = {item['object'] for item in inventory.values() if item['group'] == group}
            actual = {line.strip() for line in query.splitlines() if line.strip().endswith('.o')}
            if actual != required:
                raise RuntimeError('Linked object inventory differs from allowlist')
            if group == 'host' and 'subprojects/directx-native/libdirectx-native.a' not in query:
                raise RuntimeError('Restricted host missing its pinned static dependency')
        host = build/'libd3dmetal-native.dylib'
        if host.is_symlink() or not host.is_file():
            raise RuntimeError('Restricted host was not produced')
        shutil.copyfile(host, out/host.name)
        includes = ['-I'+str(source/'include'), '-I'+str(source/'include/native/windows'),
                    '-I'+str(source/'include/native/directx'),
                    '-I'+str(source/'subprojects/directx-native/include')]
        run([tools['clang++'], '-arch', 'x86_64', '-std=c++17', '-O2', '-dynamiclib',
             '-DWIDL_EXPLICIT_AGGREGATE_RETURNS=1', '-mmacosx-version-min=14.0', *includes,
             HERE/'unix.cpp', HERE/'offscreen.cpp', '-L'+str(out), '-ld3dmetal-native',
             '-Wl,-rpath,@loader_path', '-Wl,-install_name,@rpath/caskrun_gfxt_offscreen.so',
             '-o', out/'caskrun_gfxt_offscreen.so'], scratch, env)
        for src, output, extra in (('pe.c', 'caskrun_gfxt_offscreen.dll', ['-shared']),
                                   ('fixture.c', 'gfxt-offscreen.exe', [])):
            run([tools['x86_64-w64-mingw32-gcc'], '-std=c11', '-O2', '-Wall', '-Wextra',
                 '-Werror', *extra, HERE/src, '-o', out/output], scratch, env)
        products = {p.name: digest(p) for p in out.iterdir() if p.is_file() and not p.is_symlink()}
        if set(products) != {'libd3dmetal-native.dylib', 'caskrun_gfxt_offscreen.so',
                             'caskrun_gfxt_offscreen.dll', 'gfxt-offscreen.exe'}:
            raise RuntimeError('Unexpected build artifacts')
        metadata = {}
        for name in ('libd3dmetal-native.dylib', 'caskrun_gfxt_offscreen.so'):
            path = out/name
            dependencies = run(['/usr/bin/otool', '-L', path], scratch, env).decode()
            commands = run(['/usr/bin/otool', '-l', path], scratch, env).decode()
            header = run(['/usr/bin/otool', '-hv', path], scratch, env).decode()
            if 'TWOLEVEL' not in header:
                raise RuntimeError('A built object lacks two-level namespace binding')
            if any(word in dependencies for word in ('libd3dshared', 'D3DMetal.framework')):
                raise RuntimeError('A built object has an unexpected Apple dependency')
            metadata[name] = {'dependencies': dependencies.replace(str(scratch), '<scratch>'),
                              'load_commands': commands.replace(str(scratch), '<scratch>'),
                              'header': header.replace(str(scratch), '<scratch>')}
        helper = metadata['caskrun_gfxt_offscreen.so']
        if 'libd3dmetal-native.dylib' not in helper['dependencies'] or '@loader_path' not in helper['load_commands']:
            raise RuntimeError('Helper host linkage or relative rpath missing')
        receipt = {'built': True, 'apple_executed': False, 'wine_executed': False,
                   'scratch': str(scratch.relative_to(PROJECT)), 'sha256': products,
                   'host_objects': inventory, 'source_archive_sha256': UTM_SHA,
                   'directx_archive_sha256': DX_SHA, 'patch_sha256': PATCH_SHA,
                   'link_metadata': metadata}
        (scratch/'build-receipt.json').write_text(json.dumps(receipt, sort_keys=True, indent=2) + '\n')
        print(json.dumps(receipt, sort_keys=True))
    except Exception:
        print('Build failed; owned scratch preserved for inspection', file=sys.stderr)
        raise


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        print('Build-only preparation failed: ' + type(error).__name__, file=sys.stderr)
        sys.exit(1)
