#!/usr/bin/env python3
"""Prepare the pinned official Sparkle SDK in the project's ignored cache."""

import argparse
import hashlib
import io
import os
from pathlib import Path, PurePosixPath
import posixpath
import shutil
import stat
import sys
import tarfile
import tempfile
import urllib.request

VERSION = "2.10.0"
ARCHIVE_URL = "https://github.com/sparkle-project/Sparkle/releases/download/2.10.0/Sparkle-2.10.0.tar.xz"
ARCHIVE_SHA256 = "c2bf58aa8387266ac179357b1415d6f2635f044da8be41042af32425dae6da0c"
MAX_ARCHIVE_BYTES = 32 * 1024 * 1024
MAX_EXPANDED_BYTES = 256 * 1024 * 1024
MAX_ENTRIES = 4096


class SDKError(Exception):
    pass


def checked_path(name):
    if not name or len(name) > 4096 or "\\" in name or any(ord(c) < 32 for c in name):
        raise SDKError("SDK archive contains an invalid path")
    path = PurePosixPath(name)
    if path.is_absolute() or ".." in name.split("/") or (path.parts and ":" in path.parts[0]):
        raise SDKError("SDK archive path escapes its directory")
    return str(path)


def verified_bytes(archive, expected_digest=ARCHIVE_SHA256):
    try:
        fd = os.open(str(archive), os.O_RDONLY | os.O_NOFOLLOW)
        with os.fdopen(fd, "rb") as source:
            metadata = os.fstat(source.fileno())
            if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > MAX_ARCHIVE_BYTES:
                raise SDKError("SDK archive is not a bounded regular file")
            data = source.read(MAX_ARCHIVE_BYTES + 1)
    except OSError as error:
        raise SDKError("SDK archive is missing or unreadable") from error
    if len(data) > MAX_ARCHIVE_BYTES or hashlib.sha256(data).hexdigest() != expected_digest:
        raise SDKError("SDK archive checksum verification failed")
    return data


def archive_manifest(data):
    members = {}
    directories = {"."}
    expanded = 0
    count = 0
    saw_root = False
    try:
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:xz") as archive:
            for member in archive:
                count += 1
                if count > MAX_ENTRIES:
                    raise SDKError("SDK archive contains too many entries")
                name = checked_path(member.name)
                if name == ".":
                    if not member.isdir() or saw_root:
                        raise SDKError("SDK archive has an invalid root entry")
                    saw_root = True
                    continue
                if name in members:
                    raise SDKError("SDK archive contains duplicate paths")
                if member.type not in (tarfile.REGTYPE, tarfile.AREGTYPE, tarfile.DIRTYPE, tarfile.SYMTYPE):
                    raise SDKError("SDK archive contains an unsupported entry type")
                if member.mode & 0o7000 or member.size < 0 or (not member.isfile() and member.size != 0):
                    raise SDKError("SDK archive contains unsafe permissions or size")
                expanded += member.size
                if expanded > MAX_EXPANDED_BYTES:
                    raise SDKError("SDK archive expands beyond its size limit")
                members[name] = member
                for parent in PurePosixPath(name).parents:
                    directories.add(str(parent))
                if member.isdir():
                    directories.add(name)
    except (tarfile.TarError, EOFError, OSError) as error:
        raise SDKError("SDK archive could not be read") from error
    if not members:
        raise SDKError("SDK archive is empty")
    for name in directories - {"."}:
        member = members.get(name)
        if member is not None and not member.isdir():
            raise SDKError("SDK archive writes through a non-directory parent")
    for name, member in members.items():
        if member.issym():
            link = member.linkname
            if not link or "\\" in link or any(ord(c) < 32 for c in link) or PurePosixPath(link).is_absolute():
                raise SDKError("SDK archive contains an unsafe symbolic link")
            target = posixpath.normpath(posixpath.join(posixpath.dirname(name), link))
            if target == ".." or target.startswith("../") or ":" in target.split("/")[0]:
                raise SDKError("SDK symbolic link escapes its directory")
            parent = posixpath.dirname(name) or "."
            if target == "." or target == parent or parent.startswith(target + "/"):
                raise SDKError("SDK symbolic link points to an ancestor directory")
            _resolve_link(target, members, directories, {name})
    return members, directories


def _resolve_link(target, members, directories, visited):
    """Resolve archive links without consulting any path on the host."""
    parts = PurePosixPath(target).parts
    for index in range(1, len(parts) + 1):
        current = str(PurePosixPath(*parts[:index]))
        member = members.get(current)
        if member is not None and member.issym():
            if current in visited or len(visited) >= 40:
                raise SDKError("SDK archive contains a symbolic-link cycle")
            joined = posixpath.normpath(posixpath.join(posixpath.dirname(current), member.linkname, *parts[index:]))
            if joined == ".." or joined.startswith("../") or joined.startswith("/"):
                raise SDKError("SDK symbolic link escapes its directory")
            return _resolve_link(joined, members, directories, visited | {current})
    if target not in members and target not in directories:
        raise SDKError("SDK archive contains a broken symbolic link")
    return target


def validate_cache(sdk, data):
    members, directories = archive_manifest(data)
    try:
        root_mode = sdk.lstat().st_mode
    except OSError as error:
        raise SDKError("SDK directory is missing") from error
    if not stat.S_ISDIR(root_mode):
        raise SDKError("SDK cache is not an independent directory")
    expected = set(members) | (directories - {"."})
    actual = set()
    pending = [sdk]
    while pending:
        directory = pending.pop()
        try:
            children = list(os.scandir(directory))
        except OSError as error:
            raise SDKError("SDK cache could not be enumerated") from error
        for child in children:
            path = Path(child.path)
            name = path.relative_to(sdk).as_posix()
            actual.add(name)
            if name not in expected:
                raise SDKError("SDK cache contains an unexpected file")
            mode = child.stat(follow_symlinks=False).st_mode
            if name in directories:
                if not stat.S_ISDIR(mode):
                    raise SDKError("SDK cache directory was replaced")
                pending.append(path)
            elif members[name].issym():
                if not stat.S_ISLNK(mode) or os.readlink(path) != members[name].linkname:
                    raise SDKError("SDK cached symbolic link was modified")
            elif not stat.S_ISREG(mode):
                raise SDKError("SDK cached file is not a regular file")
    if actual != expected:
        raise SDKError("SDK cache is incomplete")
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:xz") as archive:
        for name, member in members.items():
            if not member.isfile():
                continue
            path = sdk / name
            fd = os.open(str(path), os.O_RDONLY | os.O_NOFOLLOW)
            with os.fdopen(fd, "rb") as cached, archive.extractfile(member) as original:
                metadata = os.fstat(cached.fileno())
                if not stat.S_ISREG(metadata.st_mode) or metadata.st_size != member.size or metadata.st_mode & 0o777 != member.mode & 0o777:
                    raise SDKError("SDK cached file size or permissions were modified")
                while True:
                    expected_chunk = original.read(1024 * 1024)
                    if cached.read(len(expected_chunk) or 1) != expected_chunk:
                        raise SDKError("SDK cached file content was modified")
                    if not expected_chunk:
                        break
    return sdk / "Sparkle.framework"


def extract_sdk(data, sdk):
    members, directories = archive_manifest(data)
    work = Path(tempfile.mkdtemp(prefix=".sparkle-sdk-", dir=sdk.parent))
    try:
        for name in sorted(directories - {"."}, key=lambda value: (value.count("/"), value)):
            (work / name).mkdir(exist_ok=True)
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:xz") as archive:
            for name, member in members.items():
                if member.isfile():
                    with archive.extractfile(member) as source, (work / name).open("xb") as destination:
                        shutil.copyfileobj(source, destination, length=1024 * 1024)
                    os.chmod(work / name, member.mode & 0o777)
            for name, member in members.items():
                if member.issym():
                    os.symlink(member.linkname, work / name)
        validate_cache(work, data)
        if sdk.exists() or sdk.is_symlink():
            raise SDKError("SDK cache appeared during preparation; refusing to replace it")
        work.rename(sdk)
    finally:
        if work.exists():
            shutil.rmtree(work)


def download_archive(destination):
    request = urllib.request.Request(ARCHIVE_URL, headers={"User-Agent": "Caskrun-build"})
    fd, temporary = tempfile.mkstemp(prefix=".sparkle-download-", dir=destination.parent)
    try:
        with os.fdopen(fd, "wb") as output, urllib.request.urlopen(request, timeout=30) as response:
            count = 0
            while True:
                chunk = response.read(1024 * 1024)
                if not chunk:
                    break
                count += len(chunk)
                if count > MAX_ARCHIVE_BYTES:
                    raise SDKError("SDK download exceeds its size limit")
                output.write(chunk)
        verified_bytes(Path(temporary))
        if destination.exists() or destination.is_symlink():
            raise SDKError("SDK archive appeared during download; refusing to replace it")
        os.rename(temporary, destination)
    except OSError as error:
        raise SDKError("SDK download failed") from error
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def prepare(cache, check=False, expected_digest=ARCHIVE_SHA256):
    """The digest argument is internal test injection, never a CLI override."""
    if cache.is_symlink() or (cache.exists() and not cache.is_dir()):
        raise SDKError("SDK cache root is not an independent directory")
    if not cache.exists():
        if check:
            raise SDKError("SDK cache is missing; run preparation first")
        cache.mkdir(parents=True)
    archive = cache / ("Sparkle-" + VERSION + ".tar.xz")
    sdk = cache / ("Sparkle-" + VERSION)
    if not archive.exists() and not archive.is_symlink():
        if check:
            raise SDKError("SDK archive is missing; check mode never downloads")
        download_archive(archive)
    data = verified_bytes(archive, expected_digest)
    if not sdk.exists() and not sdk.is_symlink():
        if check:
            raise SDKError("SDK extraction is missing; check mode never writes it")
        extract_sdk(data, sdk)
    framework = validate_cache(sdk, data)
    if not framework.is_dir() or framework.is_symlink():
        raise SDKError("SDK is missing its independent Sparkle.framework directory")
    return framework


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="verify the existing cache without downloading or writing")
    parser.add_argument("--print-framework", action="store_true", help="print only the verified framework path")
    args = parser.parse_args()
    project = Path(__file__).resolve().parent.parent
    cache = project / ".build" / "dependencies"
    # Do not follow a substituted .build parent into another project or app.
    for path in (project / ".build", cache):
        if path.is_symlink():
            parser.exit(1, "Sparkle preparation failed: cache parent must not be a symbolic link\n")
    try:
        framework = prepare(cache, check=args.check)
    except (SDKError, OSError) as error:
        parser.exit(1, "Sparkle preparation failed: " + str(error) + "\n")
    print(framework if args.print_framework else "Sparkle " + VERSION + " SDK verified")


if __name__ == "__main__":
    main()
