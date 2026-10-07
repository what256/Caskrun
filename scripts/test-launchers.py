#!/usr/bin/env python3
"""Isolated, resumable manual-GUI launcher validation; Python standard library only.

Setup never installs or starts Windows software. Install and run are separate,
explicit actions. Record describes only a human-observed launcher UI; gameplay
and account authentication remain untested. No command accepts license terms.
Raw diagnostics and private process receipts live only in the ignored session.
Cleanup closes only that session's bottles and preserves them on uncertainty.
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
from dataclasses import dataclass
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import stat
import subprocess
import sys
import threading
import time
from typing import Callable

CATALOG = (
    "epic-games-store", "gog-galaxy", "battle-net", "ea-app",
    "ubisoft-connect", "itch-io-app", "rockstar-games-launcher",
)
STAGES = {"ready", "running", "failed", "installer_complete", "installed"}
REASONS = {"none", "network", "download", "runtime", "setup", "timeout", "canceled", "rendering", "unknown"}
PROJECT = Path(__file__).resolve().parent.parent
SCHEMA = 1
SESSION_PATTERN = re.compile(r"[a-f0-9]{24}\Z")
MAX_JSON = 2 * 1024 * 1024


class HarnessError(Exception):
    """Messages are deliberately generic: no private paths or vendor output."""


def digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def real_path(path: Path, *, directory: bool = False) -> Path:
    path = path.absolute()
    for parent in [path, *path.parents]:
        try:
            mode = parent.lstat().st_mode
        except OSError:
            raise HarnessError("A required path is missing or unreadable") from None
        if stat.S_ISLNK(mode):
            raise HarnessError("A protected path has a symbolic-link ancestor")
    mode = path.stat().st_mode
    if not (stat.S_ISDIR(mode) if directory else stat.S_ISREG(mode)):
        raise HarnessError("A protected path has the wrong file type")
    return path.resolve()


def read_json(path: Path):
    real_path(path)
    if path.stat().st_size > MAX_JSON:
        raise HarnessError("A local receipt exceeds its size limit")
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (ValueError, UnicodeError):
        raise HarnessError("A local receipt is invalid") from None


def write_json(path: Path, value) -> None:
    real_path(path.parent, directory=True)
    if path.exists() or path.is_symlink():
        real_path(path)
    temporary = path.with_name(path.name + ".new-" + secrets.token_hex(8))
    try:
        fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8") as stream:
            json.dump(value, stream, indent=2, sort_keys=True)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)


def process_table() -> dict[int, dict]:
    """Birth stamps prevent a saved PID being confused with a reused PID."""
    try:
        result = subprocess.run(
            ["/bin/ps", "-axo", "pid=,ppid=,lstart=,command="],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5, check=False,
            env={"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LC_ALL": "C"},
        )
    except (OSError, subprocess.TimeoutExpired):
        raise HarnessError("Process inspection is unavailable") from None
    if result.returncode:
        raise HarnessError("Process inspection failed")
    table = {}
    for line in result.stdout.decode("utf-8", "replace").splitlines():
        parts = line.split(None, 7)
        if len(parts) != 8 or not parts[0].isdigit() or not parts[1].isdigit():
            continue
        table[int(parts[0])] = {"parent": int(parts[1]), "birth": " ".join(parts[2:7]), "command": parts[7]}
    if os.getpid() not in table:
        raise HarnessError("Process inspection returned incomplete data")
    return table


def collect_descendants(root_pid: int, saved: dict[str, str], table: dict[int, dict]) -> None:
    parents = {root_pid}
    parents.update(int(pid) for pid, birth in saved.items() if table.get(int(pid), {}).get("birth") == birth)
    changed = True
    while changed:
        changed = False
        for pid, info in table.items():
            if pid in parents or info["parent"] in parents:
                if str(pid) not in saved:
                    saved[str(pid)] = info["birth"]
                if pid not in parents:
                    parents.add(pid)
                    changed = True


@dataclass
class Invocation:
    value: object | None
    exit_code: int | None
    timed_out: bool
    unresolved: bool
    elapsed_ms: int
    reason: str


class Session:
    def __init__(self, project: Path, session_id: str):
        if not SESSION_PATTERN.fullmatch(session_id):
            raise HarnessError("Invalid test-session identifier")
        self.project = real_path(project, directory=True)
        self.build = real_path(self.project / ".build", directory=True)
        self.session_id = session_id
        self.root = self.build / ("launcher-validation-" + session_id)
        real_path(self.root, directory=True)
        self.manifest = read_json(self.root / "session.json")
        self._validate_owner()
        self.storage = real_path(self.root / "storage", directory=True)
        self.host = real_path(self.root / "host", directory=True)
        self.raw = real_path(self.root / "raw", directory=True)
        self.core = self._validate_executable(Path(self.manifest["core"]))
        self.wine = self._validate_executable(Path(self.manifest["wine"]))
        for key, path in (("core_sha256", self.core), ("wine_sha256", self.wine)):
            if digest(path) != self.manifest[key]:
                raise HarnessError("A selected executable changed; preserve the session for review")
        self.receipts = self.build / "launcher-receipts"
        real_path(self.receipts, directory=True)
        self.receipt = self.receipts / (session_id + ".json")
        self.summary = read_json(self.receipt)

    def _validate_owner(self) -> None:
        metadata = self.root.stat()
        marker = read_json(self.root / ".caskrun-test-owner.json")
        m = self.manifest
        if (m.get("schema") != SCHEMA or m.get("session") != self.session_id
                or m.get("uid") != os.getuid() or metadata.st_uid != os.getuid()
                or [metadata.st_dev, metadata.st_ino] != m.get("root_identity")
                or marker != {"schema": SCHEMA, "session": self.session_id, "token": m.get("owner_token")}
                or not re.fullmatch(r"[a-f0-9]{64}", m.get("owner_token", ""))):
            raise HarnessError("Test-session ownership cannot be verified")

    def _validate_executable(self, path: Path) -> Path:
        path = path.absolute()
        real_path(path.parent, directory=True)
        resolved = real_path(path.resolve())
        if not path.is_relative_to(self.build) or not resolved.is_relative_to(self.build) or not os.access(path, os.X_OK):
            raise HarnessError("Executables must be runnable project build artifacts")
        if resolved.is_relative_to(self.root):
            raise HarnessError("Shared executables cannot be inside disposable test storage")
        return path

    @classmethod
    def setup(cls, project: Path, core: Path, wine: Path) -> "Session":
        project = real_path(project, directory=True)
        build = project / ".build"
        build.mkdir(mode=0o700, exist_ok=True)
        real_path(build, directory=True)
        for executable in (core, wine):
            real_path(executable.absolute().parent, directory=True)
            resolved = real_path(executable.resolve())
            if not executable.absolute().is_relative_to(build) or not resolved.is_relative_to(build) or not os.access(executable, os.X_OK):
                raise HarnessError("Executables must be runnable project build artifacts")
        for sibling in ("wineboot", "wineserver"):
            candidate = wine.parent / sibling
            # Independent packages may use their own relative dispatcher links.
            if not candidate.resolve().is_relative_to(build) or not os.access(candidate, os.X_OK):
                raise HarnessError("The selected Wine package is incomplete")
        session_id = secrets.token_hex(12)
        root = build / ("launcher-validation-" + session_id)
        root.mkdir(mode=0o700)
        for name in ("storage", "host", "raw", "tmp"):
            (root / name).mkdir(mode=0o700)
        for name in ("Desktop", "Documents", "Downloads", "Pictures", "Music", "Videos", "Library"):
            (root / "host" / name).mkdir(mode=0o700)
        for name in ("xdg-config", "xdg-data", "xdg-cache"):
            (root / name).mkdir(mode=0o700)
        metadata = root.stat()
        token = secrets.token_hex(32)
        write_json(root / ".caskrun-test-owner.json", {"schema": SCHEMA, "session": session_id, "token": token})
        write_json(root / "session.json", {
            "schema": SCHEMA, "session": session_id, "uid": os.getuid(),
            "owner_token": token, "root_identity": [metadata.st_dev, metadata.st_ino],
            "core": str(core.absolute()), "wine": str(wine.absolute()),
            "core_sha256": digest(core), "wine_sha256": digest(wine),
            "tracked": {}, "invocations": 0,
        })
        receipts = build / "launcher-receipts"
        receipts.mkdir(mode=0o700, exist_ok=True)
        real_path(receipts, directory=True)
        write_json(receipts / (session_id + ".json"), {
            "schema": SCHEMA, "session": session_id, "cleanup": "pending",
            "core_sha256": digest(core), "wine_sha256": digest(wine),
            "launchers": {catalog_id: {"observed_ui": "untested"} for catalog_id in CATALOG},
            "commands": [], "gameplay": "untested", "authentication": "untested",
        })
        return cls(project, session_id)

    @contextmanager
    def locked(self):
        path = self.root / ".harness-lock"
        if path.exists() or path.is_symlink():
            real_path(path)
        fd = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        try:
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                raise HarnessError("Another action is using this test session") from None
            self._validate_owner()
            yield
        finally:
            os.close(fd)

    def environment(self) -> dict[str, str]:
        # No inherited credentials, custom loaders, host Wine prefix, or GUI
        # integration caches. Wine's own bottle XDG mapping adds another layer.
        return {
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
            "HOME": str(self.host), "USER": "CaskrunTest", "LOGNAME": "CaskrunTest",
            "TMPDIR": str(self.root / "tmp") + os.sep,
            "XDG_CONFIG_HOME": str(self.root / "xdg-config"),
            "XDG_DATA_HOME": str(self.root / "xdg-data"),
            "XDG_CACHE_HOME": str(self.root / "xdg-cache"),
            "CASKRUN_HOME": str(self.storage), "CASKRUN_WINE": str(self.wine),
            "CASKRUN_CANCEL_STDIN": "1", "LANG": "en_US.UTF-8",
        }

    def save(self) -> None:
        write_json(self.root / "session.json", self.manifest)
        write_json(self.receipt, self.summary)

    def invoke(self, arguments: list[str], deadline: float = 720, cancel_grace: float = 30) -> Invocation:
        self._validate_owner()
        self.manifest["invocations"] += 1
        sequence = self.manifest["invocations"]
        stdout_path = self.raw / (f"{sequence:03d}.stdout")
        stderr_path = self.raw / (f"{sequence:03d}.stderr")
        started = time.monotonic()
        timed_out = False
        cancellation_requested = False
        self.manifest["active_invocation"] = {"sequence": sequence, "command": arguments[0]}
        self.save()
        with stdout_path.open("xb") as output, stderr_path.open("xb") as errors:
            os.chmod(stdout_path, 0o600)
            os.chmod(stderr_path, 0o600)
            try:
                process = subprocess.Popen(
                    [str(self.core), *arguments], cwd=self.root, env=self.environment(),
                    stdin=subprocess.PIPE, stdout=output, stderr=errors, start_new_session=True,
                )
            except OSError:
                raise HarnessError("The selected core could not start") from None
            expires = started + deadline
            cancellation_expires = None
            try:
                while process.poll() is None:
                    tracked_before = len(self.manifest["tracked"])
                    collect_descendants(process.pid, self.manifest["tracked"], process_table())
                    if len(self.manifest["tracked"]) != tracked_before:
                        self.save()
                    now = time.monotonic()
                    request_path = self.root / ".cancel-request.json"
                    if request_path.exists() or request_path.is_symlink():
                        request = read_json(request_path)
                        cancellation_requested = request == {"sequence": sequence, "token": self.manifest["owner_token"]}
                    if (now >= expires or cancellation_requested) and cancellation_expires is None:
                        timed_out = now >= expires
                        process.stdin.close()  # Invocation-local EOF cancellation.
                        cancellation_expires = now + cancel_grace
                    if cancellation_expires is not None and now >= cancellation_expires:
                        break
                    time.sleep(min(0.2, max(0.01, expires - now)) if cancellation_expires is None else 0.2)
            except (KeyboardInterrupt, HarnessError):
                if not process.stdin.closed:
                    process.stdin.close()
                try:
                    process.wait(timeout=cancel_grace)
                except subprocess.TimeoutExpired:
                    pass
                self.manifest["active_invocation"] = None
                self.save()
                raise
            finally:
                if not process.stdin.closed:
                    process.stdin.close()
        code = process.poll()
        self.manifest["active_invocation"] = None
        unresolved = code is None
        if unresolved:
            # Retain and reap only this still-owned child if it eventually
            # finishes. A daemon waiter never signals it or blocks the CLI.
            threading.Thread(target=process.wait, daemon=True).start()
        value = None
        if code == 0:
            try:
                value = read_json(stdout_path)
            except HarnessError:
                pass
        reason = "timeout" if timed_out else "none" if code == 0 and value is not None else "unknown"
        if code == 130:
            reason = "canceled"
        elif reason == "unknown":
            with stderr_path.open("rb") as stream:
                error_text = stream.read(65536).decode("utf-8", "replace").lower()
            for category, words in (("network", ("resolve host", "connection", "network", "tls")),
                                    ("download", ("download", "installer url")),
                                    ("runtime", ("runtime", "wine executable", "windows engine")),
                                    ("setup", ("installer", "setup"))):
                if any(word in error_text for word in words):
                    reason = category
                    break
        result = Invocation(value, code, timed_out, unresolved, int((time.monotonic() - started) * 1000), reason)
        self.summary["commands"].append({
            "command": arguments[0], "catalog": next((arg for arg in arguments if arg in CATALOG), None),
            "exit_code": code, "elapsed_ms": result.elapsed_ms,
            "timed_out": timed_out, "unresolved": unresolved, "cancellation_requested": cancellation_requested, "reason": reason,
        })
        self.save()
        return result

    def bottle_evidence(self, bottle: Path) -> dict:
        """Summarize diagnostic bytes and shell links without names or targets."""
        evidence = {"logs": {"files": 0, "bytes": 0, "sha256": []},
                    "shell_links": {"contained": 0, "external": 0}}
        logs = bottle / "logs"
        if logs.exists() or logs.is_symlink():
            real_path(logs, directory=True)
            for path in logs.iterdir():
                real_path(path)
                size = path.stat().st_size
                evidence["logs"]["files"] += 1
                evidence["logs"]["bytes"] += size
                # Large diagnostics remain local and are never loaded whole.
                if size <= 64 * 1024 * 1024:
                    evidence["logs"]["sha256"].append(digest(path))
            evidence["logs"]["sha256"].sort()
        users = bottle / "prefix" / "drive_c" / "users"
        if users.exists() or users.is_symlink():
            real_path(users, directory=True)
            for account in users.iterdir():
                if account.is_symlink() or not account.is_dir():
                    continue
                for name in ("Desktop", "Documents", "Downloads", "Pictures", "Music", "Videos", "My Documents"):
                    link = account / name
                    if link.is_symlink():
                        scope = "contained" if link.resolve().is_relative_to(self.root) else "external"
                        evidence["shell_links"][scope] += 1
        return evidence

    def collect_status(self) -> dict:
        result = self.invoke(["overview"], deadline=120)
        if result.exit_code != 0 or not isinstance(result.value, dict):
            raise HarnessError("The test library could not be inspected")
        overview = result.value
        for attempt in overview.get("catalog_attempts", []):
            catalog_id = attempt.get("catalog_id")
            if catalog_id not in CATALOG:
                raise HarnessError("An unexpected catalog entry exists in test storage")
            bottle_id = attempt.get("bottle_id", "")
            if not re.fullmatch(r"[a-f0-9-]{36}", bottle_id):
                raise HarnessError("An invalid bottle identifier exists in test storage")
            stage = attempt.get("stage")
            if stage not in STAGES:
                raise HarnessError("An unknown setup stage exists in test storage")
            self.summary["launchers"][catalog_id]["setup_stage"] = stage
            self.summary["launchers"][catalog_id]["saved_launchers"] = len(overview.get("launchers", {}).get(bottle_id, []))
            installer = attempt.get("installer_path", "")
            parts = Path(installer).parts
            if not parts or parts[0] != "installers" or Path(installer).is_absolute() or ".." in parts:
                raise HarnessError("An installer receipt escapes test storage")
            path = self.storage / "bottles" / bottle_id / installer
            real_path(path)
            if not path.is_relative_to(self.storage) or path.stat().st_size > 1024 * 1024 * 1024:
                raise HarnessError("An installer receipt is too large or outside test storage")
            actual_digest = digest(path)
            self.summary["launchers"][catalog_id]["installer_sha256"] = actual_digest
            self.summary["launchers"][catalog_id]["installer_bytes"] = path.stat().st_size
            self.summary["launchers"][catalog_id]["receipt_matches"] = actual_digest == attempt.get("installer_sha256")
            self.summary["launchers"][catalog_id]["evidence"] = self.bottle_evidence(path.parent.parent)
        self.save()
        return overview

    def cancel(self) -> None:
        """Ask the owning harness to close its own core stdin; never signal PIDs."""
        self._validate_owner()
        active = self.manifest.get("active_invocation")
        if not isinstance(active, dict) or active.get("command") not in {"install-catalog", "retry-catalog", "launch-wait"}:
            raise HarnessError("No foreground setup invocation supports cancellation")
        sequence = active.get("sequence")
        if not isinstance(sequence, int) or sequence != self.manifest["invocations"]:
            raise HarnessError("Foreground setup ownership cannot be verified")
        write_json(self.root / ".cancel-request.json", {"sequence": sequence, "token": self.manifest["owner_token"]})

    def progress(self) -> dict:
        """Read atomic core receipts without locking or changing a running test."""
        summary = json.loads(json.dumps(self.summary))
        bottles = self.storage / "bottles"
        if not bottles.exists():
            return summary
        real_path(bottles, directory=True)
        for bottle in bottles.iterdir():
            real_path(bottle, directory=True)
            attempt_path = bottle / "catalog-attempt.json"
            if not attempt_path.exists():
                continue
            attempt = read_json(attempt_path)
            catalog_id = attempt.get("catalog_id")
            stage = attempt.get("stage")
            if catalog_id not in CATALOG or stage not in STAGES:
                raise HarnessError("Unexpected setup state in isolated storage")
            entry = summary["launchers"][catalog_id]
            entry["setup_stage"] = stage
            entry["setup_has_error"] = bool(attempt.get("last_error"))
            entry["setup_has_log"] = bool(attempt.get("last_log_name"))
        return summary

    def install(self, catalog_id: str, deadline: float) -> Invocation:
        if catalog_id not in CATALOG:
            raise HarnessError("Choose one of the seven non-Steam launchers")
        overview = self.collect_status()
        attempts = [attempt for attempt in overview.get("catalog_attempts", []) if attempt.get("catalog_id") == catalog_id]
        if len(attempts) > 1:
            raise HarnessError("Multiple setup attempts require manual review")
        arguments = ["retry-catalog", catalog_id, attempts[0]["bottle_id"]] if attempts else ["install-catalog", catalog_id]
        result = self.invoke(arguments, deadline=deadline)
        self.summary["launchers"][catalog_id]["install_result"] = "completed" if result.exit_code == 0 else result.reason
        self.save()
        self.collect_status()
        return result

    def run(self, catalog_id: str) -> Invocation:
        if catalog_id not in CATALOG:
            raise HarnessError("Choose one of the seven non-Steam launchers")
        overview = self.collect_status()
        attempts = [attempt for attempt in overview.get("catalog_attempts", []) if attempt.get("catalog_id") == catalog_id]
        if len(attempts) != 1 or attempts[0].get("stage") != "installed":
            raise HarnessError("This launcher has not completed setup")
        bottle_id = attempts[0]["bottle_id"]
        launchers = overview.get("launchers", {}).get(bottle_id, [])
        if len(launchers) != 1:
            raise HarnessError("Launcher selection is ambiguous")
        launcher_id = launchers[0].get("id", "")
        if not re.fullmatch(r"[a-f0-9-]{36}", launcher_id):
            raise HarnessError("The saved launcher identifier is invalid")
        return self.invoke(["run-app", bottle_id, launcher_id], deadline=120)

    def record(self, catalog_id: str, result: str, reason: str) -> None:
        if catalog_id not in CATALOG or result not in {"rendered", "failed", "blocked"} or reason not in REASONS:
            raise HarnessError("An observation must use the supported privacy-safe labels")
        self.summary["launchers"][catalog_id].update(observed_ui=result, observation_reason=reason)
        self.save()

    def assert_idle(self, timeout: float = 15) -> None:
        table = process_table()
        live = [pid for pid, birth in self.manifest["tracked"].items() if table.get(int(pid), {}).get("birth") == birth]
        if live:
            raise HarnessError("Owned test processes remain; test storage was preserved")
        bottles = self.storage / "bottles"
        prefixes = []
        if bottles.exists() or bottles.is_symlink():
            real_path(bottles, directory=True)
            for bottle in bottles.iterdir():
                real_path(bottle, directory=True)
                prefix = bottle / "prefix"
                if prefix.exists() or prefix.is_symlink():
                    prefixes.append(real_path(prefix, directory=True))
        server = self.wine.parent / "wineserver"
        if not server.resolve().is_relative_to(self.build) or not os.access(server, os.X_OK):
            raise HarnessError("The selected Wine server cannot be verified")
        for prefix in prefixes:
            environment = self.environment()
            environment["WINEPREFIX"] = str(prefix)
            try:
                result = subprocess.run([str(server), "-w"], cwd=self.root, env=environment,
                                        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                        stderr=subprocess.PIPE, timeout=timeout, check=False)
            except (OSError, subprocess.TimeoutExpired):
                raise HarnessError("A test Wine server may still be active; storage was preserved") from None
            if result.returncode:
                raise HarnessError("A test Wine server could not be verified idle")
        # Find open files for this session only. No process is signaled by this
        # inspection; diagnostics are never copied to the sanitized receipt.
        inspector = shutil.which("lsof", path="/usr/sbin:/usr/bin:/bin:/sbin")
        if inspector is None:
            raise HarnessError("Open-file inspection is unavailable; storage was preserved")
        try:
            inspection = subprocess.run([inspector, "-nP", "-Fpc", "+D", str(self.root)],
                                        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                        timeout=60, check=False)
        except (OSError, subprocess.TimeoutExpired):
            raise HarnessError("Open-file inspection failed; storage was preserved") from None
        pids = re.findall(rb"(?m)^p([0-9]+)$", inspection.stdout)
        # The caller holds only our own .harness-lock. Exclude this exact,
        # current process; every other handle is a cleanup blocker.
        if any(int(pid) != os.getpid() for pid in pids):
            raise HarnessError("A process still holds test files; storage was preserved")
        if inspection.stderr or inspection.returncode not in {0, 1} or (inspection.returncode == 0 and not pids):
            raise HarnessError("Open-file inspection was inconclusive; storage was preserved")

    def cleanup(self) -> None:
        if not shutil.rmtree.avoids_symlink_attacks:
            raise HarnessError("Safe directory removal is unavailable on this platform")
        self._validate_owner()
        # Do not race an installer/core still receiving invocation-local EOF.
        table = process_table()
        for pid, birth in self.manifest["tracked"].items():
            info = table.get(int(pid))
            if info and info["birth"] == birth and str(self.core) in info["command"]:
                raise HarnessError("A core invocation remains active; retry cleanup after it finishes")
        result = self.invoke(["quit-all-bottles"], deadline=180)
        if result.exit_code != 0 or not isinstance(result.value, dict) or result.value.get("complete") is not True:
            self.summary["cleanup"] = "blocked"
            self.save()
            raise HarnessError("Test-bottle shutdown failed; storage was preserved")
        self.assert_idle()
        self._validate_owner()
        self.summary["cleanup"] = "verified_no_live_processes"
        self.save()
        root_identity = self.manifest["root_identity"]
        if [self.root.stat().st_dev, self.root.stat().st_ino] != root_identity:
            raise HarnessError("The test directory changed during cleanup")
        shutil.rmtree(self.root)
        self.summary["cleanup"] = "removed"
        write_json(self.receipt, self.summary)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="action", required=True)
    setup = commands.add_parser("setup", help="Create an isolated session without starting software")
    setup.add_argument("--core", type=Path, required=True)
    setup.add_argument("--wine", type=Path, required=True)
    for name in ("install", "status", "run", "record", "cleanup", "cancel"):
        command = commands.add_parser(name)
        command.add_argument("session")
        if name in {"install", "run", "record"}:
            command.add_argument("catalog", choices=CATALOG)
        if name == "install":
            command.add_argument("--deadline-seconds", type=int, default=720, choices=range(1, 1801), metavar="1..1800")
        if name == "record":
            command.add_argument("--result", choices=("rendered", "failed", "blocked"), required=True)
            command.add_argument("--reason", choices=sorted(REASONS), default="none")
    arguments = parser.parse_args(argv)
    try:
        if arguments.action == "setup":
            session = Session.setup(PROJECT, arguments.core, arguments.wine)
            print(json.dumps({"session": session.session_id, "setup": "isolated"}))
            return 0
        session = Session(PROJECT, arguments.session)
        if arguments.action == "status":
            print(json.dumps(session.progress(), sort_keys=True))
            return 0
        if arguments.action == "cancel":
            session.cancel()
            print(json.dumps({"cancel": "requested"}))
            return 0
        with session.locked():
            if arguments.action == "install":
                result = session.install(arguments.catalog, arguments.deadline_seconds)
                print(json.dumps({"catalog": arguments.catalog, "exit_code": result.exit_code, "reason": result.reason, "unresolved": result.unresolved}))
                return 0 if result.exit_code == 0 else 1
            if arguments.action == "run":
                result = session.run(arguments.catalog)
                print(json.dumps({"catalog": arguments.catalog, "exit_code": result.exit_code, "reason": result.reason}))
                return 0 if result.exit_code == 0 else 1
            if arguments.action == "record":
                session.record(arguments.catalog, arguments.result, arguments.reason)
            elif arguments.action == "cleanup":
                session.cleanup()
            print(json.dumps(session.summary, sort_keys=True))
        return 0
    except (HarnessError, OSError, KeyError, TypeError) as error:
        message = str(error) if isinstance(error, HarnessError) else "A local test operation failed; fixtures were preserved"
        print(json.dumps({"error": message}), file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        print(json.dumps({"error": "Test interrupted; invocation-local cancellation requested and fixtures preserved"}), file=sys.stderr)
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
