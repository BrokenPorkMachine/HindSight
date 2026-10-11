# HindSight Product Vision

## Thesis

Reverse engineering is slow less because analysts lack individual tools than because evidence is fragmented across them. A static analyzer knows one model of a program. A debugger knows a fleeting runtime state. An emulator knows a modeled machine. Notes know what the analyst believed. The expensive work is continuously rebuilding context between those worlds.

HindSight is a research studio that makes those worlds addressable through one investigation model.

## Who it is for

### Learner
A student or self-taught ethical hacker needs professional instrumentation without being dropped into an unexplained wall of registers and graphs. Mentor Mode explains *what changed, why the view matters, what is observed versus inferred, and what a reasonable next experiment would test*.

### Independent researcher
A solo reverse engineer needs fast navigation, reproducible experiments, automation, and useful defaults on ordinary hardware. HindSight should make a single workstation feel like a small lab.

### Professional analyst
An experienced analyst wants shortcuts rather than tutorials: synchronized views, evidence pivots, deterministic replay, queryable traces, scripting, and extensibility.

### Enterprise/research team
A team needs reproducibility, provenance, reviewable conclusions, bounded plugins, consistent project artifacts, policy controls, and eventually collaboration without sacrificing local analysis performance.

## The performance flywheel

HindSight optimizes the loop that creates understanding:

1. **Orient** — map the target and surface high-value regions.
2. **Form a hypothesis** — record what is believed and its confidence.
3. **Pivot to evidence** — inspect bytes, IR, references, runtime metadata, or machine state.
4. **Run an experiment** — attach, emulate, snapshot, step, trace, or virtualize.
5. **Compare** — static vs runtime, before vs after, snapshot vs snapshot, model vs observation.
6. **Promote the result** — convert a useful finding into a named fact with provenance.
7. **Automate repetition** — script the exact safe workflow that just worked.

The product wins when a workflow that used to involve six tools becomes one continuous investigation.

## UX principles

### One concept, many lenses
Selecting a function, symbol, address, mapped region, Objective-C method, device, or trace event should update every compatible lens rather than force the analyst to manually re-enter identity.

### Evidence first
Every strong claim should answer: where did this come from, when was it observed, which engine produced it, and how confident are we?

### Progressive depth
Default surfaces should be understandable. Expert detail should be one action away, not hidden and not forced into the first screen.

### Task-oriented layouts
No single pane arrangement is optimal for all reverse-engineering work. HindSight ships curated layouts such as Explore, Code, Control Flow, Dynamic Analysis, Runtime Linking, Pointer Auth, Hypervisor RE, Firmware & Runtime, and Machine Lab.

### Bounded by default
Expensive operations need explicit budgets. File sizes, process output, trace depth, script execution, worker concurrency, and emulator stepping should have sane ceilings rather than assuming infinite RAM or patience.

## Mentor Mode

Mentor Mode is not an AI that invents answers. Its first contract is deterministic contextual teaching:

- explain the selected representation;
- label observed, decoded, inferred, and modeled facts;
- explain the consequence of common controls;
- suggest experiments that can falsify a hypothesis;
- surface prerequisite concepts inline;
- show raw evidence on demand.

A later model-assisted mentor can sit on top of this evidence graph, but it must cite project facts rather than replace them.

## The moat

HindSight should not compete only on “whose decompiler looks prettier.” Its durable differentiation is the investigation fabric joining:

- rich static semantics;
- native debugging;
- deterministic machine execution;
- hypervisor experiments;
- evidence/provenance;
- automation/extensions;
- education that scales into expert use.

The same project model should allow a beginner to learn *why* a pointer matters and a senior researcher to trace the same pointer through authenticated binding, runtime resolution, and emulated execution.
