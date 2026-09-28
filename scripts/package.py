#!/usr/bin/env python3
"""Package a native executable, preserve license texts, and smoke-test extraction."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
import zipfile

from smoke import smoke


ROOT = Path(__file__).resolve().parent.parent
LICENSE_FALLBACKS = {
    ("rmcp", "3.5.0"): "rust-sdk-3.5.0.txt",
    ("rmcp-macros", "3.5.0"): "rust-sdk-3.5.0.txt",
    ("jsonschema-regex", "0.58.2"): "jsonschema-0.58.2.txt",
    ("jsonschema-value", "0.58.2"): "jsonschema-0.58.2.txt",
    ("uuid-simd", "0.8.0"): "simd-0.8.0.txt",
    ("vsimd", "0.8.0"): "simd-0.8.0.txt",
}
NATIVE_TARGETS = {
    "x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu",
    "aarch64-apple-darwin", "x86_64-apple-darwin",
}


def dependency_notices(target: str) -> str:
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", target],
        cwd=ROOT, text=True, encoding="utf-8",
    ))
    sections = [
        "Third-party dependency notices\n"
        "Generated from Cargo.lock and locally downloaded crate packages.\n"
        "This inventory includes build and test dependencies; not every listed crate is linked.\n"
    ]
    missing = []
    for package in sorted(metadata["packages"], key=lambda item: (item["name"], item["version"])):
        if package["source"] is None:
            continue
        directory = Path(package["manifest_path"]).parent
        paths = set()
        if package.get("license_file"):
            paths.add(directory / package["license_file"])
        for path in directory.iterdir():
            if path.is_file() and path.name.upper().startswith(("LICENSE", "COPYING", "NOTICE")):
                paths.add(path)
        fallback = LICENSE_FALLBACKS.get((package["name"], package["version"]))
        license_root = directory.resolve(strict=True)
        if not paths and fallback:
            license_root = (ROOT / "scripts" / "licenses").resolve(strict=True)
            paths.add(license_root / fallback)
        if not paths:
            missing.append(f"{package['name']} {package['version']}")
        section = [
            "=" * 78,
            f"{package['name']} {package['version']}",
            f"SPDX license: {package.get('license') or 'See license text'}",
            f"Source: {package.get('repository') or package['source']}",
        ]
        for path in sorted(paths):
            resolved = path.resolve(strict=True)
            if not resolved.is_relative_to(license_root) or not resolved.is_file():
                raise RuntimeError(f"License file escapes the package directory: {package['name']}")
            section += [f"\n--- {path.name} ---\n", resolved.read_text(encoding="utf-8", errors="strict")]
        sections.append("\n".join(section))
    if missing:
        raise RuntimeError("Missing packaged dependency license texts: " + ", ".join(missing))
    return "\n\n".join(sections) + "\n"


def package(binary: Path, target: str, output: Path) -> None:
    if target not in NATIVE_TARGETS:
        raise ValueError("Unsupported release target")
    rustc = subprocess.check_output(["rustc", "--version", "--verbose"], text=True, cwd=ROOT)
    if f"host: {target}" not in rustc.splitlines():
        raise ValueError("Release packaging must run on the matching native toolchain host")
    version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    name = f"serenity-concept2-mcp-v{version}-{target}"
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="serenity-package-") as temporary:
        temporary = Path(temporary)
        content = temporary / name
        content.mkdir()
        executable = "serenity-concept2-mcp.exe" if "windows" in target else "serenity-concept2-mcp"
        shutil.copy2(binary, content / executable)
        for source in ("README.md", "LICENSE", "CHANGELOG.md"):
            shutil.copy2(ROOT / source, content / source)
        (content / "THIRD_PARTY_NOTICES.txt").write_text(dependency_notices(target), encoding="utf-8")
        (content / "docs").mkdir()
        for source in ("setup.md", "tool-reference.md", "data-semantics.md"):
            shutil.copy2(ROOT / "docs" / source, content / "docs" / source)
        if "windows" in target:
            archive = output / f"{name}.zip"
            with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as handle:
                for path in sorted(content.rglob("*")):
                    if path.is_file():
                        handle.write(path, path.relative_to(temporary))
        else:
            archive = output / f"{name}.tar.gz"
            with tarfile.open(archive, "w:gz") as handle:
                handle.add(content, arcname=name)
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        checksum = output / f"{archive.name}.sha256"
        checksum.write_text(f"{digest}  {archive.name}\n", encoding="utf-8")
        # Re-read the archive after hashing, extract only the archive we just
        # created, and run the packaged executable outside the source tree.
        if hashlib.sha256(archive.read_bytes()).hexdigest() != digest:
            raise RuntimeError("Archive checksum verification failed")
        extracted = temporary / "extracted"
        extracted.mkdir()
        if archive.suffix == ".zip":
            with zipfile.ZipFile(archive) as handle:
                handle.extractall(extracted)
        else:
            with tarfile.open(archive) as handle:
                handle.extractall(extracted, filter="data")
        expected = {
            executable, "README.md", "LICENSE", "CHANGELOG.md", "THIRD_PARTY_NOTICES.txt",
            "docs/setup.md", "docs/tool-reference.md", "docs/data-semantics.md",
        }
        actual = {path.relative_to(extracted / name).as_posix() for path in (extracted / name).rglob("*") if path.is_file()}
        if actual != expected:
            raise RuntimeError(f"Unexpected release contents: {actual ^ expected}")
        smoke(extracted / name / executable, expected_version=version)
        print(f"Packaged and verified {archive.name}\nSHA-256: {digest}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--output", type=Path, default=ROOT / "dist")
    arguments = parser.parse_args()
    package(arguments.binary.resolve(strict=True), arguments.target, arguments.output.resolve())
