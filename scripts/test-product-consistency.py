#!/usr/bin/env python3
"""Independent, dependency-free regression tests for release identity checks."""
import pathlib
import plistlib
import runpy
import tempfile
import unittest

CHECK = runpy.run_path(str(pathlib.Path(__file__).with_name("check-product-consistency.py")),
                       run_name="hindsight_consistency_test")["check"]


def make_fixture(root):
    files = {
        "Resources/schema.sql": "PRAGMA user_version = 3;\n",
        "PlatformServices/CoordinatorService/ProjectStore.swift":
            "else if version != 3 || metadataVersion != 3 {\n",
        "scripts/validate-source.py":
            """for token in ['analysisCacheEngineVersion = "arm64-analysis-v125-schema130"']: pass\n""",
        "docs/STATUS.md":
            "HindSight 0.5.15. project SQLite schema 3. Project schema remains version 3. "
            "arm64-analysis-v125-schema130.\n",
        "docs/RELEASE_READINESS.md":
            "HindSight 0.5.15. Official Apple Silicon; Intel experimental. "
            "arm64-analysis-v125-schema130.\n",
        "docs/VALIDATION.md": "HindSight 0.5.15 arm64-analysis-v125-schema130.\n",
        "docs/MASTER_ROADMAP.md": "HindSight 0.5.15 master roadmap.\n",
        "RELEASE_ADVISORY.md": "HindSight 0.5.15 source release candidate.\n",
    }
    for name, content in files.items():
        dest = root / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(content)
    for name in ["App-Info.plist", "Coordinator-Info.plist"]:
        dest = root / "Config" / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(plistlib.dumps({
            "CFBundleShortVersionString": "0.5.15",
            "CFBundleVersion": "5915",
        }))


class ConsistencyChecks(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="hindsight-consistency-")
        self.addCleanup(self.tmp.cleanup)
        self.root = pathlib.Path(self.tmp.name)
        make_fixture(self.root)

    def test_consistent_sources(self):
        self.assertEqual(CHECK(self.root), [])

    def test_wrong_project_schema_is_rejected(self):
        p = self.root / "docs/STATUS.md"
        p.write_text(p.read_text().replace("project SQLite schema 3",
                                            "project SQLite schema 2"))
        self.assertTrue(any("SQLite schema" in e for e in CHECK(self.root)))

    def test_outdated_engine_is_rejected(self):
        p = self.root / "docs/VALIDATION.md"
        p.write_text(p.read_text().replace("v125", "v104"))
        self.assertTrue(any("VALIDATION.md" in e for e in CHECK(self.root)))

    def test_wrong_version_is_rejected(self):
        p = self.root / "RELEASE_ADVISORY.md"
        p.write_text("HindSight 0.5.13 release candidate\n")
        self.assertTrue(any("RELEASE_ADVISORY.md" in e for e in CHECK(self.root)))

    def test_unsupported_intel_requirement_is_rejected(self):
        p = self.root / "docs/RELEASE_READINESS.md"
        p.write_text(p.read_text() +
                     "every distributed executable must contain both arm64 and x86_64\n")
        self.assertTrue(any("Apple Silicon" in e for e in CHECK(self.root)))

    def test_app_coordinator_version_mismatch_is_rejected(self):
        p = self.root / "Config/Coordinator-Info.plist"
        data = plistlib.loads(p.read_bytes())
        data["CFBundleVersion"] = "9999"
        p.write_bytes(plistlib.dumps(data))
        self.assertTrue(any("bundle builds disagree" in e for e in CHECK(self.root)))


if __name__ == "__main__":
    unittest.main()
