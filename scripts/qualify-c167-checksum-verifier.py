#!/usr/bin/env python3
"""Evidence-based qualification: emulate each image's own multipoint checksum
verifier/sentinel routine (identified by exact signature) with derived state
and confirm it reaches the success path for images whose static checksum table
is valid. Bounded, corpus-driven; cases that cannot be derived are reported."""
from __future__ import annotations
import json, pathlib, re, struct, subprocess, sys, uuid
from collections import Counter

TABLE = 0x1F810
ENTRY_COUNT = 31
SIG = bytes.fromhex("98 24 a8 34 00 a2 10 b3 26 fa ff ff 36 fb ff ff 3d 08")

def w(v): return struct.pack('<H', v & 0xffff)
def d(v): return struct.pack('<I', v & 0xffffffff)

def disasm_page(worker, path, data_hash, size, addr):
    start=str(addr//2)
    cmd=[str(worker),'--page-image',str(path),data_hash,str(size),start,str(uuid.uuid4()),'ecu','c167','0','0']
    p=subprocess.run(cmd,capture_output=True)
    out=p.stdout
    idx=out.rfind(b'{\"protocol\":')
    if idx<4: return ''
    n=struct.unpack('>I',out[idx-4:idx])[0]
    obj=json.loads(out[idx:idx+n].decode('utf-8','replace'))
    return obj.get('text','')

def static_table_valid(data):
    if TABLE + ENTRY_COUNT*16 > len(data): return False
    for i in range(ENTRY_COUNT):
        start,end,stored,comp=struct.unpack_from('<IIII',data,TABLE+i*16)
        if (stored & 0xffffffff) != ((~comp) & 0xffffffff): return False
        if end < start or end >= len(data) or ((end-start+1)&1): return False
        s=0
        for a in range(start,end+1,2): s=(s+struct.unpack_from('<H',data,a)[0])&0xffffffff
        if s!=stored: return False
    return True

def derive_params(text, entry):
    # Look for state read: mov r4, [0xXXXX] near start, status writes [0xYYYY],[0xZZZZ]
    lines=[l for l in text.splitlines() if l.strip()]
    state_addr=None; status1=None; status2=None
    # parse instructions by address
    for line in lines[:40]:
        m=re.search(r'(0x[0-9a-f]+)\s+0x[0-9a-f]+\s+[0-9a-f]+\s+(.*)', line)
        if not m: continue
        ins=m.group(2).strip()
        if state_addr is None:
            mm=re.match(r'mov r4, \[(0x[0-9a-f]+)\]', ins)
            if mm: state_addr=int(mm.group(1),16)
        mm=re.match(r'mov \[(0x[0-9a-f]+)\], r4', ins)
        if mm and status1 is None and state_addr is not None:
            # first such after state set is status pointer store? pattern: store 0x1e0 to [status]
            pass
        mm=re.match(r'mov \[(0x[0-9a-f]+)\], r4', ins)
        if mm and status1 is None:
            # heuristic: first mov [mem],r4 after mov r4,#0x01e0
            status1=int(mm.group(1),16)
        elif mm and status1 is not None and status2 is None:
            status2=int(mm.group(1),16)
    # better: find pattern mov r4,#0x01e0; mov r5,#0; mov [X],r4; mov [Y],r5
    m=re.search(r'mov r4, #0x01e0\n0x[0-9a-f]+\s+0x[0-9a-f]+\s+[0-9a-f]+\s+mov r5, #0\n0x[0-9a-f]+\s+0x[0-9a-f]+\s+[0-9a-f]+\s+mov \[(0x[0-9a-f]+)\], r4\n0x[0-9a-f]+\s+0x[0-9a-f]+\s+[0-9a-f]+\s+mov \[(0x[0-9a-f]+)\], r5', text)
    if m:
        status1=int(m.group(1),16); status2=int(m.group(2),16)
    # state addr: first mov r4,[mem] in routine
    m=re.search(r'mov r4, \[(0x[0-9a-f]+)\]', text)
    if m: state_addr=int(m.group(1),16)
    # success rets address: look for pattern writing #0x01e0 back to status1/status2 then rets
    succ=None
    if status1 and status2:
        pat=rf'mov r4, #0x01e0.*?mov \[{hex(status1)}\], r4.*?mov \[{hex(status2)}\], r5.*?(0x[0-9a-f]+)\s+0x[0-9a-f]+\s+[0-9a-f]+\s+rets'
        m=re.search(pat, text, re.S)
        if m: succ=int(m.group(1),16)
    # fallback: any rets that follows status1/status2 restore to 0x01e0 near end of success checks
    if succ is None:
        # try to find "mov [status1], r4; mov [status2], r5; rets" appearing late (success) 
        # by locating all rets and picking the one preceded by restoring 0x01e0
        for mm in re.finditer(r'(0x[0-9a-f]+)\s+0x[0-9a-f]+\s+[0-9a-f]+\s+rets', text):
            a=int(mm.group(1),16)
            # check preceding lines
            pre=text[:mm.start()].splitlines()[-4:]
            pre='\n'.join(pre)
            if status1 and f'mov [{hex(status1)}], r4' in pre and status2 and f'mov [{hex(status2)}], r5' in pre:
                # ensure r4 was set to 0x01e0 recently
                if 'mov r4, #0x01e0' in text[:mm.start()].split('mov r4, #0x3800')[-1]:
                    succ=a; break
    return {'state_addr': state_addr, 'status1': status1, 'status2': status2, 'success_rets': succ}

def try_emulate(worker, path, entry, params):
    sa=params.get('state_addr'); s1=params.get('status1'); s2=params.get('status2'); succ=params.get('success_rets')
    if None in (sa,s1,s2,succ): return {'error':'incomplete_params', 'params':params}
    # sentinel values from observed verifier pattern
    seeds=bytearray()
    seeds+=d(0x00000000)   # 0x1e0
    seeds+=d(0x00003fff)   # 0x1e4
    seeds+=d(0x12345678)+d(0xffffffff-0x12345678)
    seeds+=d(0x00004000)
    seeds+=d(0x00007fff)
    seeds+=d(0x00000001)+d(0xfffffffe)
    ram=[
      (0x01e0, seeds.hex()),
      (sa, w(0).hex()),
      (0xff12, w(1<<10).hex()),
      (0xfbfe, w(0).hex()),
    ]
    ram_arg=','.join(f"0x{a:x}={b}" for a,b in ram)
    cmd=[str(worker),'--emulate-c167',str(path),hex(entry),'10000','0xfc00',hex(succ),'none',ram_arg,str(uuid.uuid4())]
    p=subprocess.run(cmd,capture_output=True)
    out=p.stdout
    idx=out.rfind(b'{\"protocol\":')
    if idx<4: return {'error':'no_json'}
    n=struct.unpack('>I',out[idx-4:idx])[0]
    obj=json.loads(out[idx:idx+n].decode('utf-8','replace'))
    return {'stop':obj.get('stop'),'pc':obj.get('pc'),'steps':obj.get('steps'),'params':params}

def main():
    import argparse, hashlib
    ap=argparse.ArgumentParser()
    ap.add_argument('corpus', type=pathlib.Path)
    ap.add_argument('--worker', type=pathlib.Path, required=True)
    ap.add_argument('--output', type=pathlib.Path)
    ap.add_argument('--limit', type=int, default=0)
    args=ap.parse_args()
    corpus=args.corpus.resolve(); worker=args.worker.resolve()
    qual=json.load(open('/tmp/c167-corpus-qualify.json')) if pathlib.Path('/tmp/c167-corpus-qualify.json').exists() else None
    records=[]
    paths=sorted({*corpus.rglob('*.bin'),*corpus.rglob('*.Bin'),*corpus.rglob('*.BIN')})
    for path in paths:
        data=path.read_bytes()
        off=data.find(SIG)
        if off<0: continue
        rel=str(path.relative_to(corpus))
        # find entry candidate via simple scan: look back for function-ish boundary? use stored logic: containing function entry candidate not stored except in worker analysis; rederive cheaply by scanning backwards for common prolog? easier: parse from worker analyze
        hsh=hashlib.sha256(data).hexdigest()
        cmd=[str(worker),'--analyze-image',str(path),hsh,str(len(data)),str(uuid.uuid4()),'ecu','c167','0','0']
        p=subprocess.run(cmd,capture_output=True); out=p.stdout
        idx=out.rfind(b'{\"protocol\":')
        if idx<4: records.append({'path':rel,'error':'analyze_failed'}); continue
        n=struct.unpack('>I',out[idx-4:idx])[0]
        a=json.loads(out[idx:idx+n].decode('utf-8','replace'))
        meta=a.get('metadata','')
        entry=None
        for line in meta.splitlines():
            if 'multipoint checksum code signature: exact fixed-byte match; containing-function entry candidate' in line:
                try: entry=int(line.rsplit(' ',1)[-1].rstrip('.'),16)
                except: pass
        if entry is None: records.append({'path':rel,'error':'no_entry'}); continue
        text=disasm_page(worker,path,hsh,len(data),entry)
        params=derive_params(text,entry)
        res=try_emulate(worker,path,entry,params)
        valid=static_table_valid(data)
        ok=('Breakpoint' in str(res.get('stop')))
        records.append({'path':rel,'entry':hex(entry),'static_valid':valid,'emulated_success':ok,'result':res})
        print(f"{rel} valid={valid} emu_ok={ok} stop={res.get('stop')}", file=sys.stderr)
        if args.limit and len(records)>=args.limit: break
    summary={'images':len(records),'emulated_success':sum(1 for r in records if r.get('emulated_success')),'static_valid':sum(1 for r in records if r.get('static_valid')),'both':sum(1 for r in records if r.get('emulated_success') and r.get('static_valid'))}
    summary['fail_reasons']=dict(Counter(str(r.get('result',{}).get('stop') or r.get('result',{}).get('error') or r.get('error')) for r in records if not r.get('emulated_success')))
    report={'schema':1,'summary':summary,'records':records}
    out=json.dumps(report,indent=2,sort_keys=True)+'\n'
    if args.output: args.output.write_text(out)
    else: sys.stdout.write(out)
    return 0

if __name__=='__main__':
    raise SystemExit(main())
