#!/usr/bin/env python3
"""Verify the exact native release asset set before creating a GitHub release."""

import argparse
import hashlib
from pathlib import Path
import re
import tomllib


ROOT = Path(__file__).resolve().parent.parent
TARGETS = {
    "x86_64-pc-windows-msvc": ".zip",
    "x86_64-unknown-linux-gnu": ".tar.gz",
    "aarch64-apple-darwin": ".tar.gz",
    "x86_64-apple-darwin": ".tar.gz",
}


def verify(directory: Path, tag: str, notes_file: Path) -> None:
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    version = manifest["package"]["version"]
    if tag != f"v{version}":
        raise ValueError("Release tag must match the Cargo.toml package version")
    archives = {f"serenity-concept2-mcp-{tag}-{target}{suffix}" for target, suffix in TARGETS.items()}
    expected = archives | {f"{name}.sha256" for name in archives}
    actual = {path.name for path in directory.iterdir()}
    if actual - {"SHA256SUMS"} != expected:
        raise ValueError(f"Unexpected or missing release assets: {sorted((actual - {'SHA256SUMS'}) ^ expected)}")
    lines = []
    for filename in sorted(archives):
        archive = directory / filename
        sidecar = directory / f"{filename}.sha256"
        if archive.is_symlink() or sidecar.is_symlink() or not archive.is_file() or not sidecar.is_file():
            raise ValueError("Release assets must be regular files")
        line = sidecar.read_text(encoding="utf-8").strip()
        digest, separator, claimed_name = line.partition("  ")
        if not separator or claimed_name != filename or not re.fullmatch(r"[a-f0-9]{64}", digest):
            raise ValueError(f"Malformed checksum sidecar: {sidecar.name}")
        if hashlib.sha256(archive.read_bytes()).hexdigest() != digest:
            raise ValueError(f"Checksum mismatch: {filename}")
        lines.append(line)
    combined = "\n".join(lines) + "\n"
    manifest_path = directory / "SHA256SUMS"
    if manifest_path.exists():
        if manifest_path.is_symlink() or not manifest_path.is_file():
            raise ValueError("SHA256SUMS must be a regular file")
        if manifest_path.read_text(encoding="utf-8") != combined:
            raise ValueError("Existing SHA256SUMS does not match the verified archives")
    changelog = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8")
    marker = f"## {version}\n"
    if marker not in changelog:
        raise ValueError("Changelog must have a section for the release version")
    notes = changelog.split(marker, 1)[1].split("\n## ", 1)[0].strip()
    repository = manifest["package"]["repository"].rstrip("/")
    notes += (
        "\n\nNative binaries are unsigned and not notarized. "
        f"See the [setup guide]({repository}/blob/{tag}/docs/setup.md) "
        "for tested build environments and installation instructions.\n"
    )
    manifest_path.write_text(combined, encoding="utf-8")
    notes_file.write_text(notes, encoding="utf-8")
    print(f"Verified {len(archives)} release archives for {tag}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, default=ROOT / "dist")
    parser.add_argument("--tag", required=True)
    parser.add_argument("--notes-file", type=Path, default=ROOT / "release-notes.md")
    args = parser.parse_args()
    verify(args.directory, args.tag, args.notes_file)
