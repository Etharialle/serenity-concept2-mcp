"""Release gate regressions: altered, substituted, and unexpected artifacts."""

import hashlib
from pathlib import Path
import tempfile
import unittest

from verify_release import TARGETS, verify


class ReleaseVerificationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="serenity-release-test-")
        self.addCleanup(self.temporary.cleanup)
        root = Path(self.temporary.name)
        self.artifacts = root / "artifacts"
        self.artifacts.mkdir()
        self.notes = root / "notes.md"
        self.archives = []
        for target, suffix in TARGETS.items():
            archive = self.artifacts / f"serenity-concept2-mcp-v0.1.0-{target}{suffix}"
            archive.write_bytes(b"synthetic archive for checksum verification only")
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            archive.with_name(archive.name + ".sha256").write_text(f"{digest}  {archive.name}\n")
            self.archives.append(archive)

    def test_exact_archives_pass_and_can_be_rechecked(self):
        verify(self.artifacts, "v0.1.0", self.notes)
        verify(self.artifacts, "v0.1.0", self.notes)
        self.assertEqual(len((self.artifacts / "SHA256SUMS").read_text().splitlines()), 4)
        self.assertIn("Native binaries are unsigned", self.notes.read_text())

    def test_modified_archive_is_rejected(self):
        self.archives[0].write_bytes(b"changed after the package smoke test")
        with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
            verify(self.artifacts, "v0.1.0", self.notes)
        self.assertFalse(self.notes.exists())

    def test_extra_unverified_archive_is_rejected(self):
        (self.artifacts / "unreviewed.zip").write_bytes(b"not built by the native matrix")
        with self.assertRaisesRegex(ValueError, "Unexpected or missing"):
            verify(self.artifacts, "v0.1.0", self.notes)

    def test_checksum_cannot_substitute_a_different_filename(self):
        sidecar = self.archives[0].with_name(self.archives[0].name + ".sha256")
        digest = hashlib.sha256(b"synthetic archive for checksum verification only").hexdigest()
        sidecar.write_text(f"{digest}  ../outside.zip\n")
        with self.assertRaisesRegex(ValueError, "Malformed checksum"):
            verify(self.artifacts, "v0.1.0", self.notes)

    def test_tag_must_match_source_version(self):
        with self.assertRaisesRegex(ValueError, "tag must match"):
            verify(self.artifacts, "v9.9.9", self.notes)


if __name__ == "__main__":
    unittest.main()
