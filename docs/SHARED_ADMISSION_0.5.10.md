# Shared resource ledger — incremental integration

The coordinator now has one thread-safe reservation ledger. Scripting and firmware dispatch both reserve from it, and release on every normal return, including error replies. Estimates: 64 MiB scripting, 192 MiB firmware. Shared budget: min(25% physical RAM, 1 GiB). Thermal serious/critical rejects new jobs. Existing priority queues remain intact.

**Scope:** Worker, dyld cache, symbolication, and pseudocode still require wiring into this same ledger. Their current independent admission logic remains active. This is not full system-wide arbitration; ledger reservations are estimates, not measured peak RSS, and currently rejected jobs return an error rather than waiting for capacity. Xcode compilation not verified here.
