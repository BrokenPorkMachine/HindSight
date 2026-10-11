# HindSight Extensions Platform — Python API v2

HindSight extensions are inspectable Python source packages executed **out of process** by the Coordinator XPC service. The SwiftUI process never imports plugin Python code.

## Packaging

Extensions may be ordinary folders or directory bundles ending in `.vgplugin`. Both contain the same files:

```text
MyExtension.vgplugin/
├── plugin.json
└── main.py
```

Create a bundle scaffold with:

```bash
python3 scripts/create-plugin.py MyExtension \
  --id ca.example.my-extension \
  --name "My Extension" \
  --bundle
```

Global extensions live under `~/Library/Application Support/HindSight/Plugins`. Project-local extensions live under `<project>/Plugins` and override a global extension with the same ID.

## Manifest surface

API v2 manifests can declare:

- read/write capabilities;
- event hooks;
- commands with keyboard shortcuts;
- dockable panels (`inspector` or `bottom`);
- typed persistent settings;
- custom analysis passes;
- debugger stop filters.

Example:

```json
{
  "id": "ca.example.plugin",
  "name": "Example",
  "version": "1.0.0",
  "api_version": 2,
  "entry": "main.py",
  "capabilities": [
    "selection.read", "analysis.read", "navigation.write",
    "settings.read", "settings.write", "state.read", "state.write",
    "analysis.custom"
  ],
  "events": ["on_symbol_selected", "on_stop"],
  "commands": [
    {"id":"open-code","title":"Open Code","shortcut":"cmd+shift+o","event":"command.open-code"}
  ],
  "panels": [
    {"id":"summary","title":"Summary","event":"panel.summary","dock":"inspector","min_width":280}
  ],
  "settings": [
    {"id":"show_addresses","title":"Show Addresses","type":"bool","default_value":"true"}
  ],
  "analysis_passes": [
    {"id":"symbol-audit","title":"Symbol Audit","event":"analysis.pass.symbol-audit"}
  ],
  "debugger_filter": {
    "signals": [5, 11],
    "reasons": ["breakpoint", "segv", "trap"]
  }
}
```

## Python API

```python
from hindsight import api as vg
```

Read surfaces are capability gated:

- `vg.selection` — `selection.read`
- `vg.analysis` — `analysis.read`
- `vg.debugger` — `debugger.read`
- `vg.settings` — `settings.read`
- `vg.state` — `state.read`

Mutation calls are converted to bounded actions and applied after Python exits:

- `vg.navigate(...)`
- `vg.breakpoint_set(address)` / `vg.breakpoint_remove(address)`
- `vg.debugger_continue()` / `vg.debugger_interrupt()` / `vg.debugger_step()`
- `vg.note_append(text)`
- `vg.setting_set(key, value)`
- `vg.state_set(key, value)`

Settings and state are persisted under HindSight Application Support, separately from extension source, and each JSON store is limited to 64 KiB. Setting writes are accepted only for keys declared in the manifest.

## Events and custom passes

`vg.event` identifies the invocation. Standard hooks include:

- `on_analysis_complete`
- `on_stop`
- `on_symbol_selected`

Custom analysis passes use manifest events such as `analysis.pass.symbol-audit` and require `analysis.custom`. They receive the same bounded analysis snapshot as other API-v2 scripts.

Debugger filters are applied before `on_stop` dispatch. A plugin can restrict stop events by signal number and/or case-insensitive reason fragments.

## Dockable views

Panels are declarative and event-backed. HindSight executes the panel event and docks its bounded textual/result output in either the contextual inspector or a bottom dock. A plugin cannot inject arbitrary SwiftUI/AppKit objects into the process.

## Security model

Extensions do not execute inside SwiftUI. Capabilities are explicit, context and output are bounded, execution is capped at 30 seconds, and at most 128 actions are accepted. No shell is invoked by the Python host. Persistent state is JSON-only and bounded. Unsupported/undeclared writes are ignored or rejected rather than silently escalating extension authority.
