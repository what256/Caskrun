#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Add the reviewed experimental x64 DXMT D3D12 DLL to a new local package.

Never runs Wine, changes a user bottle, or replaces an existing runtime/app.
Apple D3DMetal is not an input. Requires exact source/build/API receipts.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
from pathlib import Path
import shutil
import sys
import tarfile
import tempfile

sys.dont_write_bytecode = True
REPO = Path(__file__).resolve().parents[1]


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, REPO / "scripts" / filename)
    value = importlib.util.module_from_spec(spec)
    sys.modules[name] = value
    spec.loader.exec_module(value)
    return value


M = module("d3d12_engine_package", "package-wine-engine-components-dev.py")
B = module("d3d12_source_builder", "build-dxmt-d3d12-dev.py")
PROBE = module("d3d12_api_contract", "test-dxmt-d3d12-dev.py")
P = M.P
BASE_IDENTITY = "2850553486175ba0326c5d4f84ea45f3fb37a5210c6830e77d7bea5ca8f97a0e"
INPUT_HASH = "00d496abca27fd91bdef037436f19a7dcbe7a3c7899f6958ef58168fb9436707"
DLL_HASH = "c22f3717f2ff21b04d4206fe68f1c2f4e0928625d145ca52b1f35cfa9456974c"
DLL = "components/dxmt/runtime/lib/wine/x86_64-windows/d3d12.dll"


def require(condition, message):
    if not condition:
        raise P.PackageError(message)


def api_proof(draw_path, present_path):
    draw, present = M.read_json(draw_path), M.read_json(present_path)
    require(draw.get("passed") is True and draw.get("actual_host_mapping_verified") is True
            and draw.get("passed_checks") == 38 and draw.get("shader_draw_verified") is True
            and draw.get("offscreen_pixels_verified") == 256
            and draw.get("selected_sha256", {}).get("d3d12.dll") == INPUT_HASH,
            "Shader API receipt does not prove the exact reviewed candidate")
    require(present.get("passed") is True and present.get("actual_host_mapping_verified") is True
            and present.get("exit_code") == 0 and isinstance(present.get("selected_sha256"), dict)
            and INPUT_HASH in present["selected_sha256"].values(),
            "Presentation receipt does not prove the exact reviewed candidate")
    log = M.regular(present_path.parent / "result.jsonl", 4 * 1024 * 1024)
    parsed = PROBE.parse_result(log.read_bytes(), False, 0, True)
    for name in ("dxgi.dll", "winemetal.dll", "winemetal.so"):
        source = next(path for path in B.INPUTS if Path(path).name == name)
        expected = B.INPUTS[source]
        require(draw["selected_sha256"].get(name) == expected
                and expected in present["selected_sha256"].values(),
                "API receipts use a different DXMT bridge")
    return {"native_candidate_sha256": INPUT_HASH,
            "shader_summary_sha256": P.sha256(draw_path),
            "presentation_summary_sha256": P.sha256(present_path),
            "presentation_log_sha256": P.sha256(log),
            "shader_checks": 38, "shader_pixels": 256, **parsed,
            "game_compatibility_verified": False}


def validate(args):
    paths = {name: P.owned_path(getattr(args, name)) for name in
             ("base", "candidate", "draw_proof", "present_proof", "output")}
    base, output, candidate = paths["base"], paths["output"], paths["candidate"]
    require(not output.exists() and not output.is_symlink(), "Output exists; refusing to replace it")
    for name, path in paths.items():
        if name != "output":
            require(output != path and output not in path.parents and path not in output.parents,
                    "Output overlaps an input")
    require(P.tree_identity(base) == BASE_IDENTITY, "Base is not the exact reviewed V15 package")
    require(P.sha256(M.regular(candidate)) == INPUT_HASH, "D3D12 source-build candidate differs")
    record_path = candidate.with_name(candidate.stem + "-provenance.json")
    record = M.read_json(record_path)
    PROBE.validate_candidate_receipt(record, INPUT_HASH)
    for name, digest in B.INPUTS.items():
        require(P.sha256(M.regular(base / name)) == digest, "A pinned source or host bridge differs")
    for name, digest in B.PATCHES.items():
        require(P.sha256(M.regular(REPO / "runtime-patches" / name)) == digest,
                "A reviewed source patch differs")
    require(P.pe_machine(candidate) == 0x8664, "D3D12 candidate is not x64")
    return {"paths": paths, "record": record, "record_path": record_path,
            "proof": api_proof(paths["draw_proof"], paths["present_proof"])}


def license_bytes(archive, name):
    with tarfile.open(archive) as stream:
        entry = stream.getmember(name)
        require(entry.isfile() and 0 < entry.size < 1024 * 1024, "Invalid source license entry")
        return stream.extractfile(entry).read()


def package(plan):
    paths, proof = plan["paths"], plan["proof"]
    base, output = paths["base"], paths["output"]
    before = M.BUILD.REOPEN.TEXT.tree_files(base)
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".dxmt-d3d12-package-", dir=output.parent) as temporary:
        root = Path(temporary) / "runtime-package"
        P.run("/bin/cp", "-cRp", base, root)
        target = P.no_symlink_parents(root / DLL)
        shutil.copy2(paths["candidate"], target)
        converted = M.mark_wine_builtin(target, base / "components/dxmt/runtime/bin/winebuild", 0x8664)
        require(converted["original_sha256"] == INPUT_HASH and converted["runtime_sha256"] == DLL_HASH,
                "Builtin packaging changed the reviewed candidate")
        added = set()
        for name in B.PATCHES:
            relative = "source/patches/" + name
            shutil.copy2(REPO / "runtime-patches" / name, root / relative)
            added.add(relative)
        archive = root / f"source/dxmt/dxmt-{B.PIN}.tar.gz"
        headers = root / f"source/dxmt/include-native-directx-{B.HEADERS_PIN}.tar.gz"
        for relative, data in {
            "licenses/DXMT-D3D12-LICENSE": license_bytes(archive, "LICENSE"),
            "licenses/DXMT-D3D12-COPYING.LIB": license_bytes(archive, "COPYING.LIB"),
            "licenses/DXMT-D3D12-HEADERS-COPYING.MinGW-w64.txt": license_bytes(headers, "COPYING.MinGW-w64.txt"),
        }.items():
            (root / relative).write_bytes(data)
            added.add(relative)
        for source, relative in ((REPO / "scripts/build-dxmt-d3d12-dev.py", "source/dxmt/d3d12-build.py"),
                                 (plan["record_path"], "source/dxmt/d3d12-build-provenance.json")):
            shutil.copy2(source, root / relative)
            added.add(relative)
        receipt = M.read_json(root / "components.json")
        receipt["patches"].update(B.PATCHES)
        receipt["artifacts"][DLL] = {"input_sha256": INPUT_HASH, "packaged_sha256": DLL_HASH,
                                      "conversion": converted["conversion"]}
        receipt["dxmt_d3d12_predecessor_identity"] = "sha256:" + BASE_IDENTITY
        receipt["dxmt_d3d12"] = {
            "implementation": "caskrun-dxmt-d3d12-v1", "windows_architectures": ["x86_64"],
            "loading_model": "wine-builtin-runtime-view-v1", "source_revision": B.PIN,
            "input_sha256": INPUT_HASH, "packaged_dll_sha256": DLL_HASH,
            "experimental": True, "game_validated": False, "api_validation_passed": True,
            "shader_model": "5_1", "feature_level": "11_1", "dxil_supported": False,
            "validation": proof,
        }
        for component in receipt["components"]:
            if component.get("name") == "DXMT":
                component["options"]["x86_64"]["enable_d3d12"] = True
        (root / "components.json").write_text(json.dumps(receipt, indent=2) + "\n")
        with (root / "SOURCES.txt").open("a") as stream:
            stream.write("\nExperimental x64 DXMT D3D12 (not Apple D3DMetal).\n"
                         "Pinned DXMT source and DirectX headers: source/dxmt/ archives.\n"
                         "Corresponding source patches: source/patches/dxmt-e94c312f-*.patch.\n"
                         "Reproducible PE build recipe/provenance: source/dxmt/d3d12-build*.\n"
                         "LGPL-2.1-or-later and header notices: licenses/DXMT-D3D12-*.\n"
                         "Only x64 DXMT d3d12.dll is replaced; base, x86 and DXVK stay unchanged.\n"
                         "FL11_1/SM5_1 maximum; no DXIL or universal/game compatibility claim.\n"
                         "GPU draw/readback and Present/resize/recreate API evidence is recorded in components.json.\n")
        after = M.BUILD.REOPEN.TEXT.tree_files(root)
        allowed = added | {DLL, "components.json", "SOURCES.txt"}
        require(all(after.get(name) == record or name in allowed for name, record in before.items()),
                "An unrelated retained runtime entry changed")
        require(set(after) - set(before) <= allowed, "An unexpected package entry was added")
        P.audit(root)
        require(P.tree_identity(base) == BASE_IDENTITY, "Reviewed base changed while packaging")
        identity = P.tree_identity(root)
        P.publish_exclusively(root, output)
    return identity


def main():
    experiment = REPO / ".build/d3dmetal-compat-oct26"
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", type=Path, default=REPO / ".build/native-text-wine-runtime-dev-v15")
    parser.add_argument("--candidate", type=Path, default=experiment / "dxmt-d3d12/build/d3d12-native.dll")
    parser.add_argument("--draw-proof", type=Path, default=experiment / "dxmt-d3d12-shader_draw-summary.json")
    parser.add_argument("--present-proof", type=Path, default=experiment / "dxmt-present-sized-probe/summary.json")
    parser.add_argument("--output", type=Path, default=REPO / ".build/native-text-wine-runtime-dev-v16")
    parser.add_argument("--check", action="store_true")
    try:
        args = parser.parse_args()
        plan = validate(args)
        if args.check:
            print("Reviewed source, native candidate, host bridge and hardware API proofs verified; no package written")
        else:
            print("Candidate runtime identity: sha256:" + package(plan))
        return 0
    except (P.PackageError, OSError, ValueError, KeyError, tarfile.TarError, PROBE.Error) as error:
        print("D3D12 packaging refused: " + str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
