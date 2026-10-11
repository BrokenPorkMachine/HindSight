from reworkbench import api as rw

sel = rw.selection
print("event:", rw.event)
print("selection:", sel)

if rw.event == "command.open-code" and sel.get("address"):
    rw.navigate(workspace="Annotated Code", address=sel["address"], cfg_focus=sel["address"], symbol=sel.get("symbol"))
elif rw.event == "panel.summary":
    show = rw.settings.get("show_addresses", True)
    print("symbol:", sel.get("symbol") or "<none>")
    if show: print("address:", sel.get("address") or "<none>")
    rw.state_set("last_panel_symbol", sel.get("symbol"))
    if sel.get("symbol"):
        rw.custom_data_set("last_symbol", sel.get("symbol"))
elif rw.event == "analysis.pass.symbol-audit":
    lines = (rw.analysis.get("symbol_provenance") or "").splitlines()
    print("symbol provenance rows:", len(lines))
    if sel.get("address"):
        rw.annotate("Symbol audit visited this address", address=sel.get("address"), category="audit")

result = {"event": rw.event, "selection": sel, "state": rw.state}
