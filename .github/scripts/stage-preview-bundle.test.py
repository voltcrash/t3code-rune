import importlib.util
import stat
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    "stage_preview_bundle", Path(__file__).with_name("stage-preview-bundle.py")
)
staging = importlib.util.module_from_spec(spec)
spec.loader.exec_module(staging)


class StagePreviewBundleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.archive = self.root / "bundle.zip"
        self.destination = self.root / "staged"

    entries = {
        "server/dist/bin.mjs": b"server",
        "server/dist/claude-history-worker.mjs": b"history worker",
        "server/dist/client/index.html": b"index",
        "server/dist/client/asset-manifest.json": b"{}",
        "desktop/dist/rune-bundle/t3-code": b"mach-o",
        "desktop/dist/rune-bundle/rune-projects.mjs": b"worker",
    }

    def bundle(self, extra=(), missing=()):
        with zipfile.ZipFile(self.archive, "w") as bundle:
            for name, data in self.entries.items():
                if name not in missing:
                    bundle.writestr(name, data)
            for name, content in extra:
                bundle.writestr(name, content)

    def stage(self):
        staging.stage_bundle(self.archive, self.destination)

    def test_preserves_valid_bundle_layout_and_bytes(self):
        self.bundle([("server/", b""), ("server/dist/", b""), ("desktop/dist/", b""),
                     ("desktop/dist/rune-bundle/", b""), ("server/dist/chunks/helper.mjs", b"chunk")])
        self.stage()
        for name, data in self.entries.items():
            self.assertEqual((self.destination / name).read_bytes(), data)
        self.assertEqual((self.destination / "server/dist/chunks/helper.mjs").read_bytes(), b"chunk")

    def test_rejects_an_old_electron_preview(self):
        self.bundle([("desktop/dist-electron/main.cjs", b"previous desktop")],
                    missing={"desktop/dist/rune-bundle/t3-code", "desktop/dist/rune-bundle/rune-projects.mjs"})
        with self.assertRaises(ValueError):
            self.stage()
        self.assertFalse(self.destination.exists())

    def test_rejects_builder_overwrite_and_unsafe_paths_before_writing(self):
        for name in [
            "desktop/node_modules/electron-builder/cli.js",
            "desktop/package.json",
            "desktop/dist-electron/main.cjs",
            "desktop/dist/rune-bundle/package.json",
            "server/dist/../../desktop/package.json",
            "../package.json",
            "/server/dist/absolute",
            "server/dist/./alias",
            "server/dist//alias",
            "server/dist/back\\slash",
            "server/dist/file:stream",
            "server/dist/BIN.MJS",
        ]:
            with self.subTest(name=name):
                self.bundle([(name, b"untrusted")])
                with self.assertRaises(ValueError):
                    self.stage()
                self.assertFalse(self.destination.exists())

    def test_rejects_links_and_special_files(self):
        for mode in [stat.S_IFLNK, stat.S_IFIFO, stat.S_IFCHR]:
            with self.subTest(mode=mode):
                entry = zipfile.ZipInfo("server/dist/link")
                entry.create_system = 3
                entry.external_attr = (mode | 0o777) << 16
                self.bundle([(entry, b"../../../desktop/node_modules")])
                with self.assertRaises(ValueError):
                    self.stage()
                self.assertFalse(self.destination.exists())

    def test_requires_entry_points(self):
        for missing in self.entries:
            with self.subTest(missing=missing):
                self.bundle(missing={missing})
                with self.assertRaises(ValueError):
                    self.stage()
                self.assertFalse(self.destination.exists())

    def test_bounds_archive_size_expanded_size_and_entry_count(self):
        for limit in ["MAX_ARCHIVE_BYTES", "MAX_EXPANDED_BYTES", "MAX_ENTRIES"]:
            with self.subTest(limit=limit), patch.object(staging, limit, 1):
                self.bundle()
                with self.assertRaises(ValueError):
                    self.stage()
                self.assertFalse(self.destination.exists())

    def test_refuses_existing_destination(self):
        self.bundle()
        self.destination.mkdir()
        sentinel = self.destination / "trusted"
        sentinel.write_text("untouched")
        with self.assertRaises(FileExistsError):
            self.stage()
        self.assertEqual(sentinel.read_text(), "untouched")


if __name__ == "__main__":
    unittest.main()
