from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class CompletionAuditTests(unittest.TestCase):
    def read(self, rel: str) -> str:
        return (ROOT / rel).read_text(encoding="utf-8")

    def test_release_identity_is_current_in_active_native_scripts(self):
        embedded = self.read("scripts/test-embedded-worker.py")
        beta = self.read("scripts/test-beta-macos.sh")
        macos = self.read("scripts/test-macos.sh")
        server = self.read("Engines/VectorEmu/crates/vg-emu-server/src/main.rs")
        cargo = self.read("Engines/VectorEmu/crates/vg-emu-server/Cargo.toml")
        for text in (embedded, beta, macos, server, cargo):
            self.assertNotIn("0.5.0", text)
            self.assertNotIn("0.4.3", text)
        self.assertIn("0.5.7", embedded)
        self.assertIn("0.5.7", server)
        self.assertIn('version = "0.5.7"', cargo)

    def test_release_readiness_matches_active_identity(self):
        readiness = self.read("docs/RELEASE_READINESS.md")
        release_script = self.read("scripts/test-release-macos.sh")
        self.assertIn("HindSight 0.5.7", readiness)
        self.assertIn("bundle build is `5700`", readiness)
        self.assertNotIn("0.5.0", readiness)
        self.assertIn('expected stable version 0.5.7', release_script)
        self.assertIn('expected build 5700', release_script)

    def test_hfsplus_reader_is_not_a_placeholder(self):
        src = self.read("Engines/VectorEmu/crates/iphem-devices/src/hfsplus.rs")
        self.assertIn("fn parse_catalog_records", src)
        self.assertIn("fn read_fork_data_with_overflow", src)
        self.assertIn("pub fn read_file", src)
        self.assertIn("pub fn read_resource_fork", src)
        self.assertIn("HFS_EXTENTS_FILE_ID", src)
        self.assertIn("signature == HFS_PLUS_MAGIC.to_be_bytes() || signature == HFSX_MAGIC.to_be_bytes()", src)
        self.assertNotIn("placeholder data", src.lower())

    def test_emmc_data_path_uses_backing_store_and_fails_closed(self):
        src = self.read("Engines/VectorEmu/crates/iphem-devices/src/storage.rs")
        self.assertIn("handled_data", src)
        self.assertIn("self.card.read_block", src)
        self.assertIn("self.card.write_block", src)
        self.assertIn("unsupported_data_command_fails_instead_of_faking_completion", src)
        self.assertNotIn("dummy data", src.lower())

    def test_invalid_logical_immediates_are_not_fabricated(self):
        src = self.read("Engines/VectorEmu/crates/iphem-arm/src/lib.rs")
        self.assertIn("<invalid-logical-immediate", src)
        self.assertNotIn("fn imm64_from_bits", src)

    def test_dart_translation_timing_is_consumed_by_translation_path(self):
        src = self.read("Engines/VectorEmu/crates/iphem-devices/src/dart.rs")
        self.assertIn("record_timed_translation(true", src)
        self.assertIn("record_timed_translation(false", src)
        self.assertGreaterEqual(src.count("record_fault_translation();"), 5)
        self.assertNotIn("not yet consumed by walker", src)

    def test_virtualization_capabilities_do_not_advertise_mock_storage(self):
        src = self.read("PlatformServices/CoordinatorService/VirtualizationHost.swift")
        self.assertNotIn("secure-storage mock backend", src)
        self.assertNotIn("SPI/I2C stubs", src)
        self.assertNotIn("SEP mailbox stub", src)
        self.assertIn("Secure storage is intentionally unavailable", src)


if __name__ == "__main__":
    unittest.main()
