# HindSight Commercial Model

## What people should pay for

The paid value should be **saved analyst time and reproducibility**, not artificial lockouts around basic learning. A learner who becomes excellent in HindSight is future demand for the professional product.

## Proposed packaging

### HindSight Learn
Free/non-commercial learning edition. Core Mach-O navigation, foundational static views, bounded VectorEmu experiments, Mentor Mode, notes, and a limited extension surface. The point is to make the product genuinely useful before asking for money.

### HindSight Personal
For independent developers and security researchers. Full static-analysis workflows, native debugger, full local VectorEmu, automation, extension SDK, project persistence, snapshots/traces, and exportable evidence. A plausible launch band is **US$99–199/year** or a perpetual personal license with paid major upgrades; validate willingness-to-pay before fixing the number.

### HindSight Pro
For people whose job is reverse engineering. Adds advanced semantic queries, large-project indexing, richer trace/snapshot correlation, custom pipelines, batch/headless tooling, premium model-assisted Mentor/Query features where evidence can be cited, and priority updates. Pricing should be tested against professional seat value rather than student budgets.

### HindSight Team / Enterprise
Adds shared investigation artifacts, review/approval workflows, extension allowlists and signing policy, centralized configuration, audit/event export, floating/managed licensing, SSO/device management, deployment controls, support SLAs, and on-prem/isolated execution options.

## Pricing principles

- Keep the free edition credible enough to teach the workflow.
- Discount students substantially; acquire skill loyalty early.
- Charge professionals for time-saving depth, not for removing nuisance limits.
- Charge organizations for governance, collaboration, deployment, support, and reproducibility.
- Avoid usage metering for local deterministic analysis; it makes experimentation psychologically expensive.
- If cloud/model features are added, price their external compute separately and keep the core local tool functional without them.

## Defensible paid features

The strongest premium candidates are not cosmetic themes. They are expensive-to-engineer capabilities that compound analyst performance:

- static ↔ debugger ↔ emulator address correlation;
- deterministic snapshot/trace diffing;
- evidence graph queries and report generation;
- multi-target projects;
- team review and provenance;
- high-confidence type/semantic recovery;
- scalable headless automation;
- enterprise policy and extension governance;
- evidence-grounded assistant/mentor workflows.

## Go-to-market wedge

Start with Apple Silicon/Mach-O because the inherited technology is unusually deep there. Market it as the best *investigation workflow* for that domain, then generalize formats and architectures. “Everything for everyone” in v1 would erase the technical advantage and delay a product users can love.

## Validation before monetization

Instrument product analytics only with explicit consent and no target-file contents. Useful signals are time-to-first-answer, most-used pivots, failed/cancelled operations, session performance, Mentor Mode transitions, and which workflows become scripts/plugins. Pair telemetry with interviews from students, independent researchers, malware analysts, and enterprise reverse engineers.
