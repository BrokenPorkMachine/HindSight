# HindSight 0.4.5 — Investigation Collections & Resume

HindSight 0.4.5 extends the project-local investigation continuity layer without changing the analysis worker protocol, evidence schema, or project SQLite schema.

## Named investigation collections

Pinned evidence can now be organized into up to 32 named collections per artifact. A collection contains stable pin identities rather than copied analysis output. Adding an evidence card to a collection automatically preserves the underlying card as a project-local pin; removing a pin also removes stale collection membership. Each collection is capped at 64 findings.

Collections are navigation organization only. Membership does not imply severity, causality, exploitability, semantic importance, or runtime reachability.

## Resume latest evidence

Successful Source, Code, CFG, and Symbol pivots already form a bounded recent-evidence history. 0.4.5 can replay the latest pivot directly from the triage surface or Workspace menu. Replay does not append a duplicate history record.

## Storage and migration

Continuity document schema advances from 1 to 2 while retaining the existing `.vectorglass/analysis-continuity-v1.json` file. The decoder accepts schema 1 and treats the missing collection list as empty, then republishes schema 2 on the next save.

The existing protections remain in force:

- 1 MiB maximum project-local continuity document.
- 128 total pinned findings.
- 96 recent evidence pivots.
- 32 named collections.
- 64 findings per collection.
- regular-file and non-symlink requirements.
- atomic writes.
- no analyst note contents in continuity metadata.

## Compatibility

0.4.5 retains protocol ABI 1, the exact 66-field `AnalysisReport`, cache envelope schema 2, and analysis engine identity `arm64-analysis-v11-schema66`. Valid 0.4.2–0.4.4 analysis caches therefore remain eligible for reuse when source and engine identity validate.
