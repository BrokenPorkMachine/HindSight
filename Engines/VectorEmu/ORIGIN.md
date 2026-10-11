# VectorEmu Origin and Scope

VectorEmu 0.1 is extracted from the reusable execution substrate developed in iPhemeral. It intentionally copies only:

- `iphem-core` — machine, bus, memory, CPU state, scheduler/trace primitives;
- `iphem-arm` — AArch64 execution/translation machinery;
- `iphem-devices` — reusable device primitives;
- `iphem-soc` — Apple SoC configuration/device construction used by the current research targets;
- the local debugger control plane, adapted into `vg-emu-server`.

It intentionally does **not** copy the iPhemeral CLI as a whole and does not include its restore/TSS/signing, jailbreak, firmware-key catalog, SSH, device-secret, or remote-network automation workflows.

VectorEmu is a HindSight execution backend, not an attempt to hide iPhemeral inside a new name. Full iPhemeral remains a separate specialized project/backend.
