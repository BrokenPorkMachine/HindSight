# Investigation Ledger

HindSight 0.3.7 introduces a persistent case layer over the workbench. The ledger is deliberately separate from the static-analysis database and from live target state: it records **what the analyst and HindSight observed or asserted during the investigation**, not a claim that every target can be rewound.

## Provenance classes

- **Observed** — captured from an external/runtime action such as a debugger stop or explicit memory read.
- **Derived** — produced by bounded static analysis, such as a completed analysis result.
- **Correlated** — a relationship supported by explicit mapping evidence. Evidence Graph continues to own the detailed correlation model.
- **Asserted** — analyst intent or interpretation: hypotheses, bookmarks, snapshots, navigation, notes, and exports.

Hypothesis status is always an analyst judgement. Marking a hypothesis `Confirmed` does not promote it to machine observation.

## Persistence

Each HindSight project stores its ledger at:

```text
<project>/.vectorglass/investigation-ledger-v1.json
```

Writes are atomic. The serialized ledger is capped at 8 MiB and individual collections are bounded:

- 4,096 timeline entries
- 512 hypotheses
- 1,024 bookmarks
- 128 session snapshots

## Timeline replay

The Ledger can step through recorded events chronologically and reopen the stored workbench context for an event. Replay means navigation through recorded case history; it does not replay CPU execution.

## Session snapshots

A snapshot stores workbench context: active workspace/layout, address, CFG focus, search, selected symbol/artifact/target, and the current evidence string. Restoring one restores **analysis context only**. It does not rewind a native process, VectorEmu machine, or Hypervisor guest.

## `.vgcase`

The export is a single JSON-encoded case file with the `.vgcase` extension. It contains:

- case ledger
- artifact identity and SHA-256 where available
- selected static analysis evidence from the currently analyzed artifact
- bounded debugger observations
- explicit memory-read observations

The original binary is not silently copied into the case file. This keeps handoff explicit and avoids duplicating proprietary or sensitive artifacts by accident.

The export is capped at 32 MiB in the 0.3.7 format line. The case can be inspected outside HindSight as JSON; import/merge workflows remain a later schema-migration feature rather than silently overwriting an existing project.

## Reports

HindSight can export the case as:

- `.vgcase` — structured handoff bundle
- Markdown — human-readable investigation report
- PDF — deterministic text report generated from the same case representation
- JSON — raw persistent ledger

All report paths are generated from the same ledger state so provenance language remains consistent.
