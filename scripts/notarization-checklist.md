# HindSight notarization credentials checklist

This checklist covers setting up Developer ID signing and notarization for stable releases.

## Prerequisites
- [ ] Apple Developer Program membership (with Developer ID certificates)
- [ ] Xcode with command line tools installed
- [ ] Valid Developer ID Application certificate in login keychain

## Environment variables (set for stable qualification)
- [ ] `VECTORGLASS_SIGN_IDENTITY` - e.g. "Developer ID Application: Your Name (TEAMID)"
- [ ] `VECTORGLASS_DEVELOPMENT_TEAM` - 10-character team ID
- [ ] `VECTORGLASS_NOTARIZE_PROFILE` - notarytool keychain profile name

## Verification steps
1. List available identities: `security find-identity -v -p codesigning`
2. Test candidate build with credentials: `./scripts/qualify-release.sh --stable`
3. Verify stapling: `xcrun stapler validate build/DerivedData/Build/Products/Release/HindSight.app`
4. Verify Gatekeeper: `spctl -a -vv build/DerivedData/Build/Products/Release/HindSight.app`

## Notes
- Candidate builds (`--candidate`) work without notarization credentials.
- Stable builds (`--stable`) require all three env vars.
- Notarization artifacts are created as `build/HindSight-0.5.13-notarization.zip` during the process.
- Credentials must never be committed to git.
