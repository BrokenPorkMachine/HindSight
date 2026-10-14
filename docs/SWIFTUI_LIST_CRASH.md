# SwiftUI List / NSOutlineView live-resize crash

## Symptom

The app could abort on the main thread while a window was being live-resized
(most easily reproduced by resizing a window hosting a SwiftUI `List`, such as
the pre-0.5.11 Binary Diff function list). The termination was an AttributeGraph
precondition failure, not a captured Swift error:

```
#4  AG::Graph::resolve_offset ()
#5  AGGraphGetValue ()
#6  SwiftUI.DynamicViewPhase.value.getter
...
#30 SwiftUI.ViewGraph.dropHandler()
#32 outlineView ()
#36 SwiftUI.OutlineListCoordinator.outlineView(_:viewFor:item:)
#35 -[NSTableView(NSTableViewViewBased) makeViewForTableColumn:row:]
...
#45 -[NSTableView viewDidEndLiveResize] ()
#50 -[NSTableView removeFromSuperview] ()
#53 destroyTableView ()
#54 dismantleViewProvider ()
#58 AG::Node::destroy ()
#62 AG::Graph::invalidate_subgraphs ()
```

`SwiftUI.OutlineListCoordinator` is the macOS `NSOutlineView` bridge that backs
`List` (and the `Form` used by Settings). During a non-interactive resize the
outline view asks its SwiftUI row provider for a view while the enclosing view
graph is being invalidated/dismantled, so the provider reads a `DynamicViewPhase`
attribute whose offset no longer resolves and AttributeGraph aborts.

## Mitigation

The workbench does not use `List`, `OutlineGroup`, or `Form` in resizable
surfaces. Each former list was rebuilt on `ScrollView` + `LazyVStack` with
explicit `ForEach` rows and `Button` selection, which has no `NSOutlineView`
coordination path:

- Binary Diff function list (`App/BinaryDiffWorkspace.swift`)
- Investigation Ledger timeline / hypotheses / bookmarks / snapshots
  (`App/InvestigationLedgerWorkspace.swift`)
- Trace Explorer observed-transition timeline (`App/TraceExplorerWorkspace.swift`)
- Settings (`App/HindSightApp.swift`) — `Form` replaced with a `ScrollView` of
  grouped sections.

Row selection is expressed with explicit state and styling rather than the
`List(selection:)` binding, so no `NSTableView` selection/row-view lifecycle is
driven by SwiftUI.

## Regression guard

`python/tests/test_ui_density.py` fails if any `App/*.swift` reintroduces a
`List(`/`List {` or `OutlineGroup`, or if `HindSightApp.swift` reintroduces
`Form {`/`.formStyle(`. That check runs in the portable suite executed by
`scripts/validate-source.py`, so the crash surface cannot silently return.

## Boundary

This removes the known `NSOutlineView` teardown avenue. It does not claim that
every possible SwiftUI/AttributeGraph interaction is crash-free; other
AttributeGraph preconditions remain tracked as ordinary defects rather than
being suppressed.
