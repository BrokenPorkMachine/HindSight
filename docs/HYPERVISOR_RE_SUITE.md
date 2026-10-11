# Hypervisor RE Suite

The Hypervisor RE Suite is the dynamic evidence layer for the Apple SoC Virtualization Kit. It separates facts obtained from HVF-backed guest RAM/vCPU state from topology provided by modeled devices.

## Evidence classes

1. **HVF-backed facts** — guest RAM bytes, PC/SP values, vCPU exit reason.
2. **HindSight mutations** — image loads, BRK patches, restored breakpoint words, snapshots.
3. **Modeled topology** — AIC/DART/ANS/SEP/T2 MMIO regions declared by the chosen research profile.
4. **Future executable device evidence** — IRQ and DART translations will only be promoted when device models actually generate/program them.

## Bounds

- memory/disassembly request: 4 KiB
- snapshot: 1 MiB
- search window: 16 MiB
- image load: 256 MiB
- software breakpoints: 256
- trace retention: 4096 events; UI response: last 1024

These limits are intentional for the target development machine and prevent a malformed guest or accidental huge request from turning the Coordinator into an unbounded memory consumer.
