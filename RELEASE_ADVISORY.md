# Release Advisory: HindSight v0.5.13 Release Candidate

Official platform: macOS 14 or newer on Apple Silicon (`arm64`). Intel builds remain experimental and are not supplied as an official binary release.

This is a research prerelease, not a claim that the reconstruction roadmap is complete. Generated source, declaration-only headers and machine-state output retain separate availability and validation claims.

## Signing and validation

Credentialed Developer ID signing, notarization and manual release-host qualification are intentionally deferred until further notice. The candidate app uses ad-hoc signing for local integrity checks; this does not identify a trusted developer or provide notarization. Gatekeeper may block a downloaded copy. Source builds are also not automatically trusted or notarized.

Download only from this repository's GitHub releases and compare the supplied SHA-256 checksums. Checksums establish file identity, not publisher trust or absence of vulnerabilities. Review the release notes for the exact checks run and remaining limitations.

## Building from source

Open `HindSight.xcodeproj`, or run `CONFIGURATION=Release ./scripts/build-app.sh` on a supported macOS development host with Xcode and Rust installed. See [project recreation](docs/PROJECT_RECREATION.md) and [release readiness](docs/RELEASE_READINESS.md) for prerequisites and deferred gates.
