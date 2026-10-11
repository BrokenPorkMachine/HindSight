from reworkbench import api as rw
sel = rw.selection
print("event:", rw.event)
print("workspace:", sel.get("workspace"))
print("symbol:", sel.get("symbol"))
if rw.event in {"command.open-code", "panel.summary"} and sel.get("address"):
    rw.navigate(workspace="Annotated Code", address=sel["address"], cfg_focus=sel["address"], symbol=sel.get("symbol"))
result = {"event": rw.event, "selection": sel}
