# Extension Trust Model

HindSight executes extension Python out of process. Trust is a second gate in addition to capability declarations.

- `verified`: Ed25519 signature in `signature.json` validates all listed files; auto-trusted.
- `unsigned`: no signature; user must explicitly trust locally.
- `invalid`: malformed/failed signature; execution is blocked and local trust cannot override it.

Dependencies are declared by plugin ID with an optional minimum version and are rechecked at execution time. Trust never bypasses dependency failure or capability checks.

The Coordinator also applies a local fail-closed capability policy. Unknown manifest capabilities are surfaced as blocked policy status and prevent execution or enabling; adding local trust cannot grant an undeclared capability.

Annotations and custom data are extension-owned, bounded to the persistent plugin-data budget, and cannot mutate core analysis results directly.

## Signing a bundle

On macOS:

```bash
swift scripts/sign-plugin.swift MyExtension.vgplugin ~/.config/vectorglass/my-extension.ed25519
```

If the private-key file does not exist, the tool creates one with owner-only permissions. Keep that file private; `signature.json` contains only the public key and signature.
