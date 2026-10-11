import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SOURCE = (ROOT / "PlatformServices/CoordinatorService/ProjectStore.swift").read_text()


class ProjectStoreSecurityContract(unittest.TestCase):
    def test_intermediate_project_directories_reject_symlinks(self):
        self.assertIn("private func validatedDirectory(", SOURCE)
        self.assertIn("values.isSymbolicLink == true", SOURCE)
        self.assertIn("values.isDirectory == true, values.isSymbolicLink != true", SOURCE)
        self.assertIn('validatedDirectory(staging, createIfMissing: false, label: "Project staging")', SOURCE)
        self.assertIn('return try validatedDirectory(url, createIfMissing: true, label: "Project \\(name)")', SOURCE)
        self.assertIn('validatedDirectory(directory, createIfMissing: false, label: "Captured object storage")', SOURCE)

    def test_staged_capture_must_be_regular_non_symlink(self):
        self.assertIn("staging.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])", SOURCE)
        self.assertIn("stagingValues.isRegularFile == true, stagingValues.isSymbolicLink != true", SOURCE)

    def test_cache_publication_refuses_symlink_or_special_destination(self):
        self.assertIn("Analysis cache destination must be a regular file, not a symbolic link or special file.", SOURCE)
        self.assertIn("existing.isRegularFile == true, existing.isSymbolicLink != true", SOURCE)


if __name__ == "__main__":
    unittest.main()
