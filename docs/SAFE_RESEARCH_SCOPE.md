# Safe Research Scope

HindSight is built for authorized reverse engineering, debugging, malware analysis, interoperability, defensive vulnerability research, education, and hardware/software research.

## Built-in boundaries

- VectorEmu listens only on loopback.
- The built-in engine does not bundle third-party firmware, secret keys, device identities, signing tickets, exploit payloads, or persistence tooling.
- Specialized iPhemeral restore/jailbreak/signing functionality is not part of VectorEmu and remains an explicit external research backend.
- Python extensions execute out of process with declared capabilities and bounded source/context/output/runtime.
- Extension mutation actions are returned to the host and revalidated rather than receiving arbitrary in-process authority.
- Analyzed files are treated as hostile input and heavy parsing is kept outside the app process.

## Product policy direction

Future convenience features should accelerate understanding and authorized experiments, not automate credential theft, stealth, persistence on third-party systems, destructive operations, or indiscriminate exploitation. Enterprise capabilities should add governance and reproducibility rather than surveillance.
