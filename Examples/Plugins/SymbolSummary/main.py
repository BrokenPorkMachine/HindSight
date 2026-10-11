from reworkbench import api as rw

selection = rw.selection
analysis = rw.analysis
symbol = selection.get("symbol") or "<none>"
print("Selected symbol:", symbol)
if symbol != "<none>":
    for label in ("functions", "xrefs", "call_graph", "relocations", "symbol_provenance"):
        hits = [line for line in analysis.get(label, "").splitlines() if symbol.lower() in line.lower()]
        if hits:
            print(f"\n[{label}]")
            print("\n".join(hits[:50]))
result = {"symbol": symbol, "workspace": selection.get("workspace")}
