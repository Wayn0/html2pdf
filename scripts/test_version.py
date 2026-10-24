"""Exercise release version edits in isolated copies, never the working tree."""
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest


class VersionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "scripts").mkdir()
        shutil.copy(Path(__file__).with_name("version.py"), self.root / "scripts/version.py")
        (self.root / "Cargo.toml").write_text('[package]\nname = "html2pdf"\nversion = "0.1.0"\n')
        (self.root / "Cargo.lock").write_text('version = 4\n\n[[package]]\nname = "dependency"\nversion = "0.1.0"\n\n[[package]]\nname = "html2pdf"\nversion = "0.1.0"\n')

    def run_version(self, command, value):
        return subprocess.run([sys.executable, str(self.root / "scripts/version.py"), command, value], capture_output=True, text=True)

    def test_bump_updates_both_application_versions_only(self):
        result = self.run_version("bump", "0.2.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(tomllib.loads((self.root / "Cargo.toml").read_text())["package"]["version"], "0.2.0")
        locked = tomllib.loads((self.root / "Cargo.lock").read_text())["package"]
        self.assertEqual([p["version"] for p in locked], ["0.1.0", "0.2.0"])
        self.assertEqual(self.run_version("check-tag", "v0.2.0").returncode, 0)

    def test_matching_tag(self):
        self.assertEqual(self.run_version("check-tag", "v0.1.0").stdout.strip(), "0.1.0")

    def test_invalid_tags(self):
        for tag in ["0.1.0", "v0.2.0", "v0.1.0-rc.1", "v01.1.0", "v1.2.3; echo nope"]:
            with self.subTest(tag=tag):
                self.assertNotEqual(self.run_version("check-tag", tag).returncode, 0)

    def test_bad_bump_does_not_modify_files(self):
        before = {name: (self.root / name).read_bytes() for name in ["Cargo.toml", "Cargo.lock"]}
        for value in ["0.1.0", "0.0.9", "v0.2.0", "0.2", "0.2.0-rc.1", "01.2.3"]:
            with self.subTest(value=value):
                self.assertNotEqual(self.run_version("bump", value).returncode, 0)
                for name, content in before.items():
                    self.assertEqual((self.root / name).read_bytes(), content)

    def test_lockfile_mismatch_blocks_release_and_bump(self):
        path = self.root / "Cargo.lock"
        path.write_text(path.read_text().replace('name = "html2pdf"\nversion = "0.1.0"', 'name = "html2pdf"\nversion = "0.0.9"'))
        for command, value in [("check-tag", "v0.1.0"), ("bump", "0.2.0")]:
            self.assertNotEqual(self.run_version(command, value).returncode, 0)


if __name__ == "__main__":
    unittest.main()
