#!/usr/bin/env python3
"""Read PE interface tables and check Apple's redist against a Wine build.

Never loads code, changes a runtime, or treats symbol availability as ABI or
rendering proof. A passing static report still requires both live GPU probes
and proof of mapped host components before a backend can be exposed.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import struct
import sys


class AuditError(Exception):
    pass


class PE:
    """Bounded Windows import/export metadata reader, not a disassembler."""
    def __init__(self, path: Path):
        if path.stat().st_size > 128 * 1024 * 1024:
            raise AuditError("PE exceeds metadata audit limit")
        self.data = path.read_bytes()
        if self.data[:2] != b"MZ":
            raise AuditError("Missing DOS signature")
        header = self.u32(60)
        if self.slice(header, 4) != b"PE\0\0":
            raise AuditError("Missing PE signature")
        self.machine, sections = self.unpack("<HH", header + 4)
        optional_size = self.u16(header + 20)
        optional = header + 24
        magic = self.u16(optional)
        if magic not in (0x10B, 0x20B):
            raise AuditError("Unknown PE optional header")
        self.width = 8 if magic == 0x20B else 4
        directory = optional + (112 if self.width == 8 else 96)
        if directory + 16 > optional + optional_size:
            raise AuditError("Missing PE interface directories")
        self.export_rva = self.u32(directory)
        self.import_rva = self.u32(directory + 8)
        self.headers_size = self.u32(optional + 60)
        self.sections = []
        if sections > 128:
            raise AuditError("Too many PE sections")
        for i in range(sections):
            entry = optional + optional_size + i * 40
            virtual_size, rva, size, offset = self.unpack("<IIII", entry + 8)
            self.slice(offset, size)
            self.sections.append((rva, virtual_size, offset, size))

    def slice(self, offset: int, size: int) -> bytes:
        if offset < 0 or size < 0 or offset + size > len(self.data):
            raise AuditError("PE metadata points outside its file")
        return self.data[offset:offset + size]

    def unpack(self, fmt: str, offset: int):
        return struct.unpack(fmt, self.slice(offset, struct.calcsize(fmt)))

    def u16(self, offset: int) -> int:
        return self.unpack("<H", offset)[0]

    def u32(self, offset: int) -> int:
        return self.unpack("<I", offset)[0]

    def offset(self, rva: int, length: int = 1) -> int:
        if rva < self.headers_size and rva + length <= self.headers_size:
            self.slice(rva, length)
            return rva
        for start, _virtual, offset, size in self.sections:
            if start <= rva and rva + length <= start + size:
                return offset + rva - start
        raise AuditError("PE interface RVA is not file-backed")

    def string(self, rva: int) -> str:
        offset = self.offset(rva)
        end = self.data.find(b"\0", offset, min(len(self.data), offset + 1024))
        if end < 0:
            raise AuditError("PE interface name is unterminated")
        return self.data[offset:end].decode("ascii", errors="strict")

    def imports(self) -> dict[str, list[str]]:
        if not self.import_rva:
            return {}
        result = {}
        for i in range(512):
            entry = self.offset(self.import_rva + i * 20, 20)
            lookup, stamp, chain, dll_rva, iat = self.unpack("<IIIII", entry)
            if not any((lookup, stamp, chain, dll_rva, iat)):
                return result
            dll = self.string(dll_rva).lower()
            if not dll.endswith(".dll") or "/" in dll or "\\" in dll:
                raise AuditError("PE import has an invalid module name")
            names = []
            for j in range(65536):
                offset = self.offset((lookup or iat) + j * self.width, self.width)
                value = self.unpack("<Q" if self.width == 8 else "<I", offset)[0]
                if not value:
                    break
                if value & (1 << (self.width * 8 - 1)):
                    names.append("#" + str(value & 0xFFFF))
                else:
                    names.append(self.string(value + 2))
            else:
                raise AuditError("PE import table exceeds audit limit")
            result.setdefault(dll, []).extend(names)
        raise AuditError("PE module import table exceeds audit limit")

    def exports(self) -> set[str]:
        if not self.export_rva:
            return set()
        entry = self.offset(self.export_rva, 40)
        base, count, names, functions_rva, names_rva, ordinals_rva = self.unpack("<IIIIII", entry + 16)
        if count > 65536 or names > count:
            raise AuditError("PE export table exceeds audit limit")
        result = set()
        for i in range(count):
            if self.u32(self.offset(functions_rva + i * 4, 4)):
                result.add("#" + str(base + i))
        for i in range(names):
            ordinal = self.u16(self.offset(ordinals_rva + i * 2, 2))
            if ordinal >= count:
                raise AuditError("PE named export has an invalid ordinal")
            if self.u32(self.offset(functions_rva + ordinal * 4, 4)):
                result.add(self.string(self.u32(self.offset(names_rva + i * 4, 4))))
        return result


def digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def audit(component: Path, wine: Path) -> dict:
    receipt = json.loads((component / "component-receipt.json").read_text())
    if (receipt.get("schema") != 1 or receipt.get("component") != "apple-d3dmetal-user-supplied"
            or receipt.get("license_reviewed_sha256") != receipt.get("license_sha256")):
        raise AuditError("Component has no exact-license-reviewed private import receipt")
    if not receipt.get("license_sha256"):
        raise AuditError("Component has no license identity")
    for record in receipt["files"]:
        relative = Path(record["path"])
        if relative.is_absolute() or ".." in relative.parts:
            raise AuditError("Component receipt has an unsafe path")
        path = component / relative
        if record["type"] == "file" and (path.is_symlink() or digest(path) != record["sha256"]):
            raise AuditError("Component no longer matches its import receipt")
        if record["type"] == "symlink" and (not path.is_symlink()
                or str(path.readlink()) != record["target"]):
            raise AuditError("Component link no longer matches its import receipt")
    apple = component / "lib/wine/x86_64-windows"
    providers = {p.name.lower(): p for p in wine.iterdir() if p.is_file() and p.suffix.lower() == ".dll"}
    providers.update({p.name.lower(): p for p in apple.iterdir() if p.is_file() and p.suffix.lower() == ".dll"})
    reports = []
    missing = 0
    apisets = 0
    for module in sorted(apple.glob("*.dll")):
        image = PE(module)
        if image.machine != 0x8664:
            raise AuditError("Apple component is not x86_64")
        for dll, symbols in sorted(image.imports().items()):
            if dll.startswith(("api-ms-", "ext-ms-")):
                reports.append({"module": module.name, "dependency": dll,
                                "status": "requires_wine_apiset_resolution"})
                apisets += 1
                continue
            provider = providers.get(dll)
            if provider is None:
                reports.append({"module": module.name, "dependency": dll,
                                "status": "missing_module", "missing_symbols": sorted(set(symbols))})
                missing += len(symbols) or 1
                continue
            provider_pe = PE(provider)
            if provider_pe.machine != 0x8664:
                raise AuditError("Wine provider has the wrong Windows architecture")
            absent = sorted(set(symbols) - provider_pe.exports())
            missing += len(absent)
            reports.append({"module": module.name, "dependency": dll,
                            "status": "missing_exports" if absent else "direct_exports_present",
                            "provider_sha256": digest(provider), "missing_symbols": absent})
    return {"schema": 1, "component_image_sha256": receipt["source_image_sha256"],
            "static_missing_count": missing, "apiset_contracts_requiring_live_resolution": apisets,
            "direct_pe_imports_satisfied": missing == 0 and apisets == 0,
            "forwarded_exports_resolved": False, "unix_loader_abi_verified": False,
            "mac_driver_surface_abi_verified": False, "d3d11_gpu_verified": False,
            "d3d12_gpu_verified": False, "backend_available": False, "dependencies": reports}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--component", type=Path, required=True)
    parser.add_argument("--wine-windows", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = audit(args.component, args.wine_windows)
        print(json.dumps(result, sort_keys=True, indent=2))
        return 0 if result["direct_pe_imports_satisfied"] else 1
    except (AuditError, OSError, UnicodeError, ValueError, KeyError, TypeError) as error:
        message = str(error) if isinstance(error, AuditError) else type(error).__name__
        print("D3DMetal static audit refused: " + message, file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
