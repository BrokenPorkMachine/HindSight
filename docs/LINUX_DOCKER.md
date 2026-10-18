# Linux-native processing with Docker

HindSight's Rust/C analysis worker can run as a one-shot Linux container on
`linux/amd64` or `linux/arm64`. The same worker, decoder and length-prefixed JSON
protocol are used; this is not a second analysis implementation. Docker builds
for its host architecture by default. On macOS, Docker runs it in its Linux VM.
Cross-architecture Docker emulation is useful for tests, not native qualification.

This packages **static processing**, not the Swift/macOS app or a complete remote
server. No HTTP listener, authentication layer, debugger or guest VM is exposed.
The future app-hosted gateway may schedule Linux jobs through a trusted host-side
adapter; automatic coordinator scheduling is not implemented by this addition.
The app continues to own projects, authorization and results. Never mount a Docker
socket into the analysis image or expose this administrative launcher directly to
remote clients.

## Build and use

Install Docker with a local Linux daemon (Docker Desktop works on macOS), and
Python 3.9+ on the invoking host. From the repository root:

```sh
docker build -t hindsight-worker:local .
python3 scripts/docker-worker.py health
python3 scripts/docker-worker.py analyze /absolute/path/to/program
python3 scripts/docker-worker.py page /absolute/path/to/program --start-word 0
```

The launcher hashes the selected regular file, mounts only that file read-only,
and verifies the response's source identity. Analysis/paging accept `--format`
(`auto`, `macho`, `elf`, `pe`, `coff`, `pdb`, `raw`), `--architecture`,
`--load-address` and `--file-offset`. Raw mappings require an explicit architecture
and appropriate load address. Format/CPU support remains bounded by the worker;
an accepted file is not a claim of complete decompilation.

For an admitted function in a thin AArch64 Mach-O, use exact start/end addresses
from the analysis function list:

```sh
python3 scripts/docker-worker.py reconstruct /path/to/arm64.macho --start 0xSTART --end 0xEND
python3 scripts/docker-worker.py interpret /path/to/arm64.macho --start 0xSTART --end 0xEND --inputs 0,42,0,0,0,0,0,0
```

Replace the address placeholders with real hexadecimal addresses. `interpret`
uses the bounded modeled machine-state interpreter, **not native execution** of
the input. Reconstruction stays review-only/blocked and generated C stays
uncompiled until separate validation gates pass. The wrapper does not yet import
prototype/type catalogs. Native Apple runtime/SDK discovery, Foundation/libobjc,
Mach/XPC, Apple Compression codecs and Apple virtualization remain on qualified
macOS hosts. Shared static Mach-O/dyld metadata parsing is available on Linux;
Linux is not advertised as a Darwin runtime or an Apple behavioral oracle.

Output is readable JSON by default; `--framed` preserves the worker's u32
big-endian length-prefixed JSON for an existing consumer. Diagnostics/progress
stay on stderr. Worker errors return a nonzero launcher exit status, including
errors delivered in a valid frame. Nothing is uploaded to ZenSoftware.

## Runtime restrictions

The launcher disables networking, uses a read-only root filesystem, drops all
Linux capabilities, disallows privilege escalation, and applies 2 CPU / 1 GiB
memory / 64-process limits. Output is bounded to one 32 MiB frame and 256 KiB of
diagnostics. The default wall-clock deadline is 180 seconds (`--timeout 1..3600`).
Timeouts/cancellation forcibly remove only that invocation's randomly named
container; daemon/connection failure can require manual cleanup, which is
reported. Each job auto-removes on normal completion.

The image defaults to UID/GID 65532. The launcher uses the invoking non-root
UID/GID so owner-only captures remain readable on Linux; a root invoker still
runs the worker as 65532. Input ancestors must permit access by that identity.
Inputs are capped at 8 GiB, symlink leaves and special files are rejected, parent
symlinks are resolved, and the worker revalidates SHA-256. There is no writable
host mount, published port, host PID namespace, `ptrace`, device or Docker socket.
Images are never pulled implicitly at job launch; `--image` selects an already
built/pulled **trusted** image, not an image supplied by a remote user.

Containers share a Linux kernel and are not a replacement for the isolated VMs
required to execute untrusted targets. This setup deliberately does not enable
native launch/attach or VM execution. Those need separate consent, session,
isolation and capability qualification. Prefer a rootless daemon where supported.
Docker access itself is privileged administrative access; source mounts refer to
the daemon host's filesystem, so the launcher is intended for a local daemon.
See [Docker runtime controls](https://docs.docker.com/engine/containers/run/) and
[Docker daemon security](https://docs.docker.com/engine/security/).

## Verification and distribution

```sh
python3 -m unittest discover -s python/tests -p test_docker_worker.py -v
docker build --target test -t hindsight-worker:test .
docker build -t hindsight-worker:local .
python3 scripts/test-docker-worker.py
```

The test stage runs Rust worker/protocol tests and existing framed-worker,
multi-architecture Mach-O/raw/firmware, ELF/PE/COFF/PDB fixture checks inside Linux.
The runtime smoke test checks owner-only read-only input, source identity,
analysis, paging, reconstruction, interpretation, framing and invalid-input
rejection using the restricted launcher. Only original synthetic fixtures are
interpreted. No imported target is launched.

`.github/workflows/linux-worker.yml` defines separate native amd64 and arm64
Linux jobs ([runner labels](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)).
Adding the jobs is not evidence they have passed; release qualification requires
their actual results. The Docker test stage needs more than 2 GiB of free space
for existing capture headroom checks. Only the minimal runtime stage should be
distributed; test/build stages contain toolchains and source. `.dockerignore`
allow-lists the necessary source/fixture paths and excludes local projects,
credentials, `.git` and build outputs from the build context.

The default compiler image matches `rust-toolchain.toml` and Cargo uses the
committed lockfile. `--build-arg RUST_IMAGE=...` allows an administrator to use a
trusted equivalent cached image; its compiler must match the pinned toolchain.
Base OS package updates are not byte-reproducibly pinned. Before public image
publication, pin reviewed base digests, refresh/security-scan OS packages,
record platform-specific image digests and test both architectures. This change
does not publish an image or satisfy the remote server's audit/push gate.

Local checkpoint on 2026-10-18: the Linux amd64 image built and passed the
test stage under Docker emulation on Apple Silicon (253 worker and five protocol
tests plus worker, multi-format and ELF/PE fixtures). The restricted runtime
smoke passed health, owner-only input, analysis, paging, reconstruction,
interpretation, framed output and invalid-input rejection. Native Linux amd64
and arm64 runner qualification is still open.
