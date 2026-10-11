#!/usr/bin/env python3
import argparse, json, re
from pathlib import Path
p=argparse.ArgumentParser(description='Create a HindSight API-v2 extension scaffold')
p.add_argument('directory'); p.add_argument('--id', required=True); p.add_argument('--name', required=True); p.add_argument('--bundle', action='store_true', help='create a .vgplugin directory bundle')
a=p.parse_args()
if not re.fullmatch(r'[A-Za-z0-9._-]{1,128}',a.id): raise SystemExit('invalid plugin id')
root=Path(a.directory).expanduser()
if a.bundle and root.suffix.lower() != '.vgplugin': root=root.with_name(root.name+'.vgplugin')
root.mkdir(parents=True,exist_ok=False)
manifest={
  "id":a.id,"name":a.name,"version":"1.0.0","api_version":2,"entry":"main.py",
  "capabilities":["selection.read","analysis.read","navigation.write","settings.read","settings.write","state.read","state.write","analysis.custom"],
  "events":["on_symbol_selected"],
  "commands":[{"id":"open-code","title":"Open Selected Address","shortcut":"cmd+shift+o","event":"command.open-code"}],
  "panels":[{"id":"summary","title":"Selection Summary","event":"panel.summary","description":"Docked view for the current selection.","dock":"inspector","min_width":280}],
  "settings":[{"id":"show_addresses","title":"Show Addresses","type":"bool","default_value":"true","description":"Include addresses in panel output."}],
  "analysis_passes":[{"id":"symbol-audit","title":"Symbol Audit","event":"analysis.pass.symbol-audit","description":"Run a custom read-only analysis pass over the current analysis snapshot."}],
  "debugger_filter":{"signals":[5,11],"reasons":["breakpoint","segv","trap"]}
}
(root/'plugin.json').write_text(json.dumps(manifest,indent=2)+'\n')
(root/'main.py').write_text('''from hindsight import api as vg\n\nsel = vg.selection\nprint("event:", vg.event)\nprint("selection:", sel)\n\nif vg.event == "command.open-code" and sel.get("address"):\n    vg.navigate(workspace="Annotated Code", address=sel["address"], cfg_focus=sel["address"], symbol=sel.get("symbol"))\nelif vg.event == "panel.summary":\n    show = vg.settings.get("show_addresses", True)\n    print("symbol:", sel.get("symbol") or "<none>")\n    if show: print("address:", sel.get("address") or "<none>")\n    vg.state_set("last_panel_symbol", sel.get("symbol"))\nelif vg.event == "analysis.pass.symbol-audit":\n    lines = (vg.analysis.get("symbol_provenance") or "").splitlines()\n    print("symbol provenance rows:", len(lines))\n\nresult = {"event": vg.event, "selection": sel, "state": vg.state}\n''')
print(root)
