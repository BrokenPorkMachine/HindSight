# VectorEmu Local Debug Protocol

## Transport

`vg-emu-server` listens on `127.0.0.1:<port>` only (default `37920`). Each request and response is one JSON object followed by a newline. The server starts paused.

The CoordinatorService launches the process directly (no shell), caps captured engine output, and caps debugger replies. The built-in UI exposes a conservative command subset even though the engine contains additional research helpers.

## Lifecycle

```bash
vg-emu-server --rom /path/to/rom.bin --soc t8020 --port 37920
```

ROM input must be between 1 byte and 64 MiB. Supported SoC selectors in the current beta are `a7`, `a8`, `a9`/`s8000`, `t8010`, `t8015`, `t8020`, `t8027`, `t8030`, and `t8101`.

## Coordinator-exposed commands

- `hello` — protocol/features handshake.
- `status` — execution/attach state and stop reason.
- `attach` / `detach` — debugger session state.
- `pause` / `run` / `step` — bounded execution control.
- `reset` — reconstruct the deterministic target state from the original ROM.
- `registers` / `cpu_state` — architectural state.
- `regions` — mapped bus regions.
- `read_memory` — bounded memory observation.
- `disassemble` — decode instructions at an address.
- `breakpoint_add`, `breakpoint_remove`, `breakpoints` — execution breakpoints.
- `trace` — recent deterministic trace records.
- `header`, `convert` — bounded format/conversion helpers inherited from the debugger plane.
- `shutdown` — request clean engine termination.

`write_memory`, assembler helpers, and write-watch features exist in the lower-level engine code but are intentionally not exposed through the first HindSight coordinator allowlist.

## Examples

```json
{"command":"attach"}
{"command":"registers"}
{"command":"disassemble","address":"0x100000000","count":16}
{"command":"breakpoint_add","address":"0x100000120"}
{"command":"run","steps":10000}
{"command":"trace","count":128}
```

## Determinism contract

A reset rebuilds the target from the same ROM and SoC descriptor and re-establishes its reset PC/stack/exception-level state. Future versions should include an engine build hash, model revision, seed, snapshot ID, and trace schema revision in every session header so two runs can be compared cryptographically rather than by convention.

## Protocol evolution

The current line-oriented JSON protocol is deliberately inspectable during local pre-release development. Before remote/team execution is introduced, the protocol should be versioned, authenticated, length-framed, capability-negotiated, and optionally transported through a mutually authenticated channel. HindSight must not turn the local research control plane into an unauthenticated network service.
