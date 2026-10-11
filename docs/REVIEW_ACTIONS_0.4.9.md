# HindSight 0.4.9 — Review Actions & Follow-Up Queue

HindSight 0.4.9 extends the project-local review workflow with explicit follow-up tasks. The analysis worker remains unchanged: protocol ABI 1, the exact 66-field `AnalysisReport`, cache envelope schema 2, project database schema v1, and `arm64-analysis-v11-schema66` are preserved.

## Follow-up queue

Each investigation collection can carry bounded analyst-authored follow-ups. A follow-up has an Open/Completed state, a bounded title and optional context note, creation/completion timestamps, and up to 16 exact evidence identities captured from the collection when the action was created. The identity snapshot provides provenance context; the follow-up is not itself evidence and is never interpreted as vulnerability, severity, exploitability, correctness, reachability, or semantic importance.

The Review surface keeps the queue separate from the Review Journal. Open work is shown first, completed work remains available, actions can be reopened, and removal is explicit. The generated Markdown handoff includes open follow-ups plus a count of completed actions so another analyst can see unresolved work without reading workflow intent into the evidence corpus.

## Persistence and bounds

Analysis continuity advances to schema 5 and continues to read schemas 1–4. Follow-ups share the existing `.vectorglass/analysis-continuity-v1.json` project-local store and its 1 MiB total ceiling. Additional limits are 128 follow-ups globally, 32 per collection, 256 UTF-8 bytes for titles, 1 KiB for optional notes, and 16 related evidence identities per follow-up. State is still atomically published and the store retains regular-file/symlink hardening.

Deleting a collection deletes its collection-scoped follow-ups. Unpinning evidence does not silently delete a follow-up: its bounded identity snapshot remains workflow provenance even when the underlying pin is no longer in the current pin set.

## Compatibility

0.4.9 does not change analysis semantics or cache admission. Compatible 0.4.2–0.4.8 schema-2 analysis caches remain reusable when their existing source and engine identity checks succeed.
