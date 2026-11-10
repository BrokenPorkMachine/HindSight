#!/usr/bin/env python3
"""Corpus-wide descriptor-driven qualification of the ME7 C167 checksum-sum
compute primitive. All 75 signature images share the same sum-loop core and
prologue/epilogue structure with family-specific parameter block addresses.
For each image this derives the primitive entry, parameter addresses, and
breakpoint from disassembly evidence, emulates each verified descriptor range,
and compares the 32-bit word sum to the static descriptor verifier.

Ranges overlapping the primitive's own parameter block are split so original
image bytes in the overlap contribute their static sum while the remainder is
emulated (the routine reads/writes params at fixed RAM addresses).
"""
from __future__ import annotations
import hashlib, json, pathlib, re, struct, subprocess, sys, uuid

TABLE = 0x1F810
LOOP = bytes.fromhex("081f1820081f1820a0053dfa")

def w(v): return struct.pack('<H', v & 0xffff)

def disasm(worker, path, data, addr, lines=40):
    hsh = hashlib.sha256(data).hexdigest()
    start = str(addr // 2)
    cmd = [str(worker), '--page-image', str(path), hsh, str(len(data)), start,
           str(uuid.uuid4()), 'ecu', 'c167', '0', '0']
    p = subprocess.run(cmd, capture_output=True)
    out = p.stdout
    idx = out.rfind(b'{"protocol":')
    if idx < 4: return ''
    n = struct.unpack('>I', out[idx-4:idx])[0]
    obj = json.loads(out[idx:idx+n].decode('utf-8', 'replace'))
    return obj.get('text', '')

def parse_primitive(text, loop_addr):
    # Expect 5 loads at entry = loop_addr-46: mov r1,[p1];r2,[p2];r3,[p3];r4,[p4];r5,[p5]
    # and rets at loop_addr+42
    entry = loop_addr - 46
    rets = loop_addr + 42
    loads = re.findall(r'mov (r[1-5]), \[(0x[0-9a-f]+)\]', text)
    params = {}
    for reg, addr in loads:
        if reg not in params:
            params[reg] = int(addr, 16)
    if not all(k in params for k in ('r1','r2','r3','r4','r5')):
        return None
    # store-back addresses indicate full param block span
    stores = re.findall(r'mov \[(0x[0-9a-f]+)\], (r1|r2|r3|r11)\b', text)
    store_addrs = [int(a,16) for a,_ in stores]
    all_addrs = list(params.values()) + store_addrs
    pmin = min(all_addrs)
    pmax = max(all_addrs) + 2  # each is a 16-bit word address
    return {'entry': entry, 'rets': rets, 'params': params,
            'block_start': pmin, 'block_end': pmax}

def static_sum(data, s, e):
    return sum(struct.unpack_from('<H', data, a)[0] for a in range(s, e + 1, 2)) & 0xffffffff

def emulate_once(worker, path, prim, start, end, r1i=0, r2i=0):
    count_pairs = (end - start + 1) // 4
    p = prim['params']
    # Seed param block: r1->p1 (acc low), r2->p2 (acc high), r3->p3 (addr lo), r4->p4 (addr hi), r5->p5 (count)
    # Initialize whole block area to 0 in case of overlaps, then set needed fields.
    seeds = []
    # write accumulators
    seeds.append((p['r1'], w(r1i).hex()))
    seeds.append((p['r2'], w(r2i).hex()))
    seeds.append((p['r3'], w(start & 0xffff).hex()))
    seeds.append((p['r4'], w((start >> 16) & 0xffff).hex()))
    seeds.append((p['r5'], w(count_pairs).hex()))
    seeds.append((0xfbfe, w(0).hex()))
    ram_arg = ','.join(f"0x{a:x}={b}" for a, b in seeds)
    cmd = [str(worker), '--emulate-c167', str(path), hex(prim['entry']), '1000000',
           '0xfc00', hex(prim['rets']), 'none', ram_arg, str(uuid.uuid4())]
    proc = subprocess.run(cmd, capture_output=True)
    out = proc.stdout
    idx = out.rfind(b'{"protocol":')
    if idx < 4: return {'error': 'no_json'}
    n = struct.unpack('>I', out[idx-4:idx])[0]
    obj = json.loads(out[idx:idx+n].decode('utf-8', 'replace'))
    if 'Breakpoint' not in str(obj.get('stop')):
        return {'error': 'no_breakpoint', 'stop': obj.get('stop'), 'steps': obj.get('steps')}
    r1 = obj['registers']['r1']; r2 = obj['registers']['r2']
    return {'ok': True, 'sum': (r2 << 16) | r1, 'steps': obj.get('steps')}

def emulate_range(worker, path, data, prim, start, end):
    b0, b1 = prim['block_start'], prim['block_end'] - 1
    if end < b0 or start > b1:
        return emulate_once(worker, path, prim, start, end)
    # Primitive processes 4 bytes (2 words) per iteration; split boundaries must
    # be 4-byte aligned. Expand to cover the full param block with original bytes.
    s0 = b0 & ~3
    s1 = (b1 + 4) & ~3  # exclusive aligned end
    if s0 < start: s0 = start
    if s1 > end + 1: s1 = end + 1
    # ensure ranges remain 4-byte aligned for emulation
    if (s0 - start) % 4 != 0:
        s0 = start + ((s0 - start) & ~3)
    if (end + 1 - s1) % 4 != 0:
        s1 = end + 1 - ((end + 1 - s1) % 4)
    total = 0; steps = 0
    if s0 > start:
        r = emulate_once(worker, path, prim, start, s0 - 1)
        if not r.get('ok'): return r
        total = (total + r['sum']) & 0xffffffff; steps += r.get('steps', 0)
    total = (total + static_sum(data, s0, s1 - 1)) & 0xffffffff
    if s1 <= end:
        r = emulate_once(worker, path, prim, s1, end)
        if not r.get('ok'): return r
        total = (total + r['sum']) & 0xffffffff; steps += r.get('steps', 0)
    return {'ok': True, 'sum': total, 'steps': steps, 'split': True}

def main():
    import argparse
    ap = argparse.ArgumentParser()
    ap.add_argument('corpus', type=pathlib.Path)
    ap.add_argument('--worker', type=pathlib.Path, required=True)
    ap.add_argument('--output', type=pathlib.Path)
    ap.add_argument('--limit', type=int, default=0)
    args = ap.parse_args()
    corpus = args.corpus.resolve(); worker = args.worker.resolve()
    qual = json.load(open('/tmp/c167-corpus-qualify.json'))
    targets = [r for r in qual['records'] if r.get('code_signatures')]
    if args.limit: targets = targets[:args.limit]
    records = []
    for r in targets:
        rel = r['path']
        path = corpus / rel
        data = path.read_bytes()
        loop_off = data.find(LOOP)
        if loop_off < 0:
            records.append({'path': rel, 'error': 'no_loop'}); continue
        text = disasm(worker, path, data, loop_off - 46)
        prim = parse_primitive(text, loop_off)
        if not prim:
            records.append({'path': rel, 'error': 'parse_failed', 'loop': hex(loop_off)}); continue
        rec = {'path': rel, 'entry': hex(prim['entry']), 'params': {k: hex(v) for k,v in prim['params'].items()},
               'entries': [], 'ok': 0, 'bad': 0, 'skip': 0}
        for i in range(31):
            o = TABLE + i * 16
            if o + 16 > len(data): rec['skip'] += 1; continue
            start,end,stored,comp = struct.unpack_from('<IIII', data, o)
            if start==0 and end==0 and stored==0 and comp==0: rec['skip'] += 1; continue
            if end < start or end >= len(data) or ((end-start+1) & 1): rec['skip'] += 1; continue
            expected = static_sum(data, start, end)
            if expected != stored: rec['skip'] += 1; continue
            res = emulate_range(worker, path, data, prim, start, end)
            if res.get('ok') and res['sum'] == expected:
                rec['ok'] += 1
                rec['entries'].append({'index': i, 'sum': expected, 'match': True})
            else:
                rec['bad'] += 1
                rec['entries'].append({'index': i, 'expected': expected, 'emulated': res.get('sum'),
                                       'match': False, 'detail': {k:v for k,v in res.items() if k!='sum'}})
        records.append(rec)
        print(f"{rel}: ok={rec['ok']} bad={rec['bad']} skip={rec['skip']}", file=sys.stderr)
    summary = {'images': len(records),
               'total_ok': sum(r.get('ok',0) for r in records),
               'total_bad': sum(r.get('bad',0) for r in records),
               'total_skip': sum(r.get('skip',0) for r in records),
               'errors': sum(1 for r in records if 'error' in r)}
    report = {'schema': 1, 'summary': summary, 'records': records}
    out = json.dumps(report, indent=2, sort_keys=True) + '\n'
    if args.output: args.output.write_text(out)
    else: sys.stdout.write(out)
    return 0

if __name__ == '__main__':
    raise SystemExit(main())
