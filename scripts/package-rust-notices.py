#!/usr/bin/env python3
"""Collect dependency-provided notices for the locked ARM64 core build."""

import argparse
import json
from pathlib import Path
import subprocess


def collect(project: Path) -> str:
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--manifest-path", str(project / "core/Cargo.toml"),
        "--format-version", "1", "--locked", "--filter-platform", "aarch64-apple-darwin",
    ], text=True))
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    root = metadata["resolve"]["root"]
    reachable, pending = set(), [root]
    while pending:
        package = pending.pop()
        if package in reachable:
            continue
        reachable.add(package)
        pending.extend(dep["pkg"] for dep in nodes[package]["deps"])
    packages = sorted((p for p in metadata["packages"] if p["id"] in reachable and p["id"] != root),
                      key=lambda p: (p["name"], p["version"]))
    result = ["Caskrun Rust dependency notices\n",
              "Versions are locked in Cargo.lock. These notices apply to their respective components.\n"]
    for package in packages:
        directory = Path(package["manifest_path"]).parent
        notices = sorted(p for p in directory.iterdir()
                         if p.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE"))
                         and not p.is_symlink() and p.is_file())
        if not notices:
            raise ValueError("Dependency has no packaged licence text: " + package["name"])
        result.extend(["\n" + "=" * 72 + "\n",
                       package["name"] + " " + package["version"] + "\n",
                       "Declared licence: " + (package.get("license") or "See notices") + "\n",
                       "https://crates.io/crates/" + package["name"] + "/" + package["version"] + "\n"])
        for notice in notices:
            if notice.stat().st_size > 1024 * 1024:
                raise ValueError("Dependency notice exceeds size limit: " + package["name"])
            result.extend(["\n--- " + notice.name + " ---\n", notice.read_text(encoding="utf-8"), "\n"])
    return "".join(result)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    project = Path(__file__).resolve().parent.parent
    output = args.output.absolute()
    build = (project / ".build").resolve(strict=True)
    parent = output.parent.resolve(strict=True)
    if build not in (parent, *parent.parents) or output.is_symlink() or output.exists():
        raise SystemExit("Choose a new notices file inside the project build directory.")
    text = collect(project)
    with output.open("x", encoding="utf-8") as file:
        file.write(text)
    print("Locked ARM64 dependency notices collected.")


if __name__ == "__main__":
    main()
