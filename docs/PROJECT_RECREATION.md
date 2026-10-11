# HindSight Research Tool project recreation

The permanent product title is **HindSight Research Tool**. The application bundle is **HindSight.app**, its executable is **HindSight**, and the Xcode project, workspace, app target, and shared scheme are named **HindSight**.

The recreated project includes the existing static analysis, debugger, scripting, plugin, emulation, virtualization, evidence, and investigation features. It carries forward the bounded worker queues, queued cancellation, per-plugin state locking, source validation, and SwiftUI compiler workarounds. Concrete workspace type boundaries prevent the reproduced Swift 6.4 SIL-generation recursion crash; Debug compilation also retains the per-file build setting.

## Clean source recreation

After committing the verified source, run:

```sh
python3 scripts/package-source.py
```

This creates a ZIP and a new source folder under `build/recreated/<commit>/HindSightResearchTool`. The archive contains only the committed source. It excludes Git metadata, Xcode user state, DerivedData, compiled artifacts, and local module caches. Script executable permissions are restored in the clean folder.

Open `HindSight.xcodeproj` and select the `HindSight` scheme, or build from that folder with:

```sh
./scripts/build-app.sh
```

The app is produced at `build/DerivedData/Build/Products/Debug/HindSight.app`. The build uses project-local DerivedData, which avoids reusing the earlier damaged global build database. Delete only that project's generated `build/DerivedData` folder when a fresh build is needed; source recreation never copies it.

VectorEmu Release builds retain symbols because the pinned Rust toolchain's stripping step produced a host macro dylib with a misaligned LINKEDIT string pool on macOS 27. The corresponding toolchain defect is tracked in [rust-lang/rust#157750](https://github.com/rust-lang/rust/issues/157750). This setting does not change release optimization.

## Existing projects and extensions

Bundle and XPC identifiers, `.vectorglass` project metadata, plugin persistent data, `VG` model types, and `reworkbench` C/Rust ABI symbols retain their existing identities for compatibility. New scripts can use `from hindsight import api as vg`. The `vectorglass` and `reworkbench` Python module aliases continue to work for existing extensions.

## Verification limits

The recreated source passed topology/protocol validation and 196 Python tests (6 skipped). Universal Debug and Release builds, nested ad-hoc signature verification, and embedded worker health tests passed on the local macOS 27/Xcode 27 toolchain. Both app and embedded executables contain arm64 and x86_64 slices. Developer ID distribution signing and notarization remain separate release checks. Passing source tests does not establish that every feature is complete or that every possible defect has been eliminated. The remaining throughput and memory-admission limits are recorded in [the dispatch audit](APPLE_SILICON_DISPATCH_AUDIT.md).

Some managed execution environments cannot start the Swift compiler's nested SDK macro sandbox. `HINDSIGHT_SWIFT_DISABLE_SANDBOX=1 ./scripts/test-app-typecheck.sh` is an opt-in verification option for that environment. The local full builds used the equivalent command-line override, `./scripts/build-app.sh 'OTHER_SWIFT_FLAGS=$(inherited) -disable-sandbox'`. It changes compiler subprocess behavior only; the default compiler behavior and application runtime settings remain controlled by the project.
