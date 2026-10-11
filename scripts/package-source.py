#!/usr/bin/env python3
"""Recreate the committed source in a clean folder and a distributable ZIP."""
from pathlib import Path
import shutil
import stat
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def git(*arguments: str) -> str:
    return subprocess.check_output(["git", "-C", str(ROOT), *arguments], text=True).strip()


def main() -> None:
    if Path(git("rev-parse", "--show-toplevel")).resolve() != ROOT:
        raise SystemExit("Run this script from the HindSight Git checkout.")
    if git("diff", "--name-only") or git("diff", "--cached", "--name-only"):
        raise SystemExit("Commit the tracked source changes before recreating the project.")
    revision = git("rev-parse", "--short=12", "HEAD")
    build = ROOT / "build"
    recreated = build / "recreated"
    recreated.mkdir(parents=True, exist_ok=True)
    destination = recreated / revision
    archive = build / f"HindSightResearchTool-{revision}-source.zip"
    if destination.exists():
        raise SystemExit(f"This revision is already recreated at {destination}.")
    subprocess.run(["git", "-C", str(ROOT), "archive", "--format=zip",
                    "--prefix=HindSightResearchTool/", f"--output={archive}", "HEAD"], check=True)
    staging = Path(tempfile.mkdtemp(prefix="HindSight-", dir=recreated))
    try:
        with zipfile.ZipFile(archive) as source:
            for entry in source.infolist():
                relative = Path(entry.filename)
                if relative.is_absolute() or ".." in relative.parts or relative.parts[0] != "HindSightResearchTool":
                    raise ValueError(f"Invalid archive path: {entry.filename}")
                if any(part in {".git", "build", "DerivedData", "xcuserdata", "target"} for part in relative.parts):
                    raise ValueError(f"Local build or checkout state was tracked: {entry.filename}")
                mode = entry.external_attr >> 16
                if stat.S_ISLNK(mode):
                    raise ValueError(f"Source archive symlinks are not supported: {entry.filename}")
                path = staging / relative
                if entry.is_dir():
                    path.mkdir(parents=True, exist_ok=True)
                else:
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(source.read(entry))
                    path.chmod(mode & 0o777 or 0o644)
        staging.rename(destination)
    except BaseException:
        shutil.rmtree(staging)
        raise
    print(f"Clean project: {destination / 'HindSightResearchTool' / 'HindSight.xcodeproj'}")
    print(f"Source archive: {archive}")
    print(f"Source revision: {git('rev-parse', 'HEAD')}")


if __name__ == "__main__":
    main()
