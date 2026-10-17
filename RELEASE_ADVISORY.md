# Release Advisory: HindSight v0.5.12 Release Candidate

This prerelease provides the source package for review. It is not a notarized macOS download. Stable distribution still requires the Developer ID signing and notarization gate and the Intel and Apple Silicon host qualification matrix described in [release readiness](docs/RELEASE_READINESS.md).

## Code Signing Requirements

Stable non-source builds require proper code signing to run:

### For Non-Source Builds (Installer/Download):
- **Download only from official HindSight sources** to avoid quarantine
- **Verify digital signatures** using the provided verification tools
- **Consider building from source** if you need unsigned code for development

### For Source Code Builds:
- **No quarantine required** when building from source
- **Follow these steps** to build safely:
  1. Clone the repository: `git clone https://github.com/BrokenPorkMachine/HindSight.git`
  2. Run the build script: `./build.sh`
  3. The built application will not be quarantined

### Security Notes:
- This application contains signed Mach-O binaries with embedded certificates
- Building from source provides full transparency of the code
- Source builds avoid any potential quarantine or notarization issues

## Recommended Actions

### If you want to run HindSight safely:
1. **Build from source** (recommended for production use)
2. **Verify the digital signature** of the binary
3. **Check the release notes** for any security updates

### If you prefer the pre-built binary:
1. **Download from official sources only**
2. **Right-click the installer** and select "Open" to bypass quarantine
3. **Trust the developer** in Security & Privacy preferences

## Additional Resources

For detailed build instructions and source code verification, refer to:
- [Project Recreation Guide](docs/PROJECT_RECREATION.md)
- [Release Qualification Preflight](docs/VALIDATION.md)
- [Documentation](docs/README.md)

## Contact

If you have questions about code signing or quarantine, please refer to the official HindSight documentation or contact the development team.
