#!/usr/bin/env python3
"""Corpus-wide descriptor-driven qualification of ME7 C167 checksum-sum
compute primitives. Signature images share a DPP-style sum-loop core with
family-specific parameter blocks; some non-signature images use an EXTS-style
sum-loop primitive. For each qualifying image this derives the primitive entry,
parameter addresses, and breakpoint from disassembly/byte evidence, emulates
each verified descriptor range, and compares the 32-bit word sum to the static
descriptor verifier.

Ranges overlapping the primitive's own parameter block are split so original
image bytes in the overlap contribute their static sum while the remainder is
emulated (the routine reads/writes params at fixed RAM addresses).
"""
from __future__ import annotations
import argparse, hashlib, json, pathlib, re, struct, subprocess, sys, uuid

TABLE = 0x1F810
LOOP_DPP = bytes.fromhex("081f1820081f1820a0053dfa")
LOOP_EXTS = re.compile(rb"\xdc\x14\x08\x1f\x18\x20\xdc\x14\x08\x1f\x18\x20\xa0\x05\x3d[\x00-\xff]")

def w(v): return struct.pack('<H', v & 0xffff)

def disasm(worker, path, data, addr, lines=40):
    hsh = hashlib.sha256(data).hexdigest()
    start = str(max(0, addr // 2))
    cmd = [str(worker), '--page-image', str(path), hsh, str(len(data)), start,
           str(uuid.uuid4()), 'ecu', 'c167', '0', '0']
    p = subprocess.run(cmd, capture_output=True)
    out = p.stdout
    idx = out.rfind(b'{"protocol":')
    if idx < 4: return ''
    n = struct.unpack('>I', out[idx-4:idx])[0]
    obj = json.loads(out[idx:idx+n].decode('utf-8', 'replace'))
    return obj.get('text', '')

def parse_loads_stores(text):
    loads = re.findall(r'mov (r[1-5]), \[(0x[0-9a-f]+)\]', text)
    params = {}
    for reg, addr in loads:
        if reg not in params:
            params[reg] = int(addr, 16)
    stores = re.findall(r'mov \[(0x[0-9a-f]+)\], (r1|r2|r3|r4|r11)\b', text)
    store_addrs = [int(a,16) for a,_ in stores]
    return params, store_addrs

def parse_primitive_dpp(text, loop_addr):
    # Expect 5 loads at entry = loop_addr-46: mov r1,[p1];r2,[p2];r3,[p3];r4,[p4];r5,[p5]
    # and rets at loop_addr+42
    entry = loop_addr - 46
    rets = loop_addr + 42
    params, store_addrs = parse_loads_stores(text)
    if not all(k in params for k in ('r1','r2','r3','r4','r5')):
        return None
    all_addrs = list(params.values()) + store_addrs
    pmin = min(all_addrs)
    pmax = max(all_addrs) + 2
    return {'entry': entry, 'rets': rets, 'params': params,
            'block_start': pmin, 'block_end': pmax, 'kind': 'dpp'}

def parse_primitive_exts(data, body_start):
    # body: dc14 081f1820 dc14 081f1820 a005 3dXX (16 bytes)
    search_from = max(0, body_start - 0x40)
    window = data[search_from:body_start]
    idx = window.rfind(b'\xf2\xf1')
    if idx < 0:
        return None
    entry = search_from + idx
    params = {}
    pos = entry
    for reg in ('r1','r2','r3','r4','r5'):
        if pos + 4 > len(data):
            return None
        b = data[pos:pos+4]
        expect = bytes([0xf2, 0xf1 + (ord(reg[1]) - ord('1'))])
        if b[:2] != expect:
            return None
        params[reg] = struct.unpack_from('<H', data, pos + 2)[0]
        pos += 4
    cmp_at = body_start + 12
    rets = data.find(b'\xdb\x00', cmp_at, cmp_at + 0x60)
    if rets < 0:
        return None
    # store-back addresses inside the primitive (between cmp and rets) define block span
    store_addrs = []
    for m in re.finditer(b'\xf6[\xf1-\xfb]..', data[cmp_at:rets+2]):
        store_addrs.append(struct.unpack_from('<H', data, cmp_at + m.start() + 2)[0])
    all_addrs = list(params.values()) + store_addrs
    pmin = min(all_addrs)
    pmax = max(all_addrs) + 2
    return {'entry': entry, 'rets': rets, 'params': params,
            'block_start': pmin, 'block_end': pmax, 'kind': 'exts'}

def find_primitive(worker, path, data):
    loop_off = data.find(LOOP_DPP)
    if loop_off >= 0:
        text = disasm(worker, path, data, loop_off - 46)
        prim = parse_primitive_dpp(text, loop_off)
        if prim:
            return prim
    # try exts-loop variant
    m = LOOP_EXTS.search(data)
    if m:
        prim = parse_primitive_exts(data, m.start())
        if prim:
            # sanity: confirm load params from disasm without widening block span
            text = disasm(worker, path, data, prim['entry'])
            params, _stores = parse_loads_stores(text)
            if all(k in params for k in ('r1','r2','r3','r4','r5')):
                prim['params'] = params
                all_addrs = list(params.values()) + [a for a in range(prim['block_start'], prim['block_end'])]
                prim['block_start'] = min(all_addrs)
                prim['block_end'] = max(all_addrs) + 1
            return prim
    return None

def static_sum(data, s, e):
    return sum(struct.unpack_from('<H', data, a)[0] for a in range(s, e + 1, 2)) & 0xffffffff

def emulate_once(worker, path, prim, start, end, r1i=0, r2i=0):
    count_pairs = (end - start + 1) // 4
    p = prim['params']
    seeds = []
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
    s0 = b0 & ~3
    s1 = (b1 + 4) & ~3
    if s0 < start: s0 = start
    if s1 > end + 1: s1 = end + 1
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
    ap = argparse.ArgumentParser()
    ap.add_argument('corpus', type=pathlib.Path)
    ap.add_argument('--worker', type=pathlib.Path, required=True)
    ap.add_argument('--output', type=pathlib.Path)
    ap.add_argument('--limit', type=int, default=0)
    ap.add_argument('--non-signature-only', action='store_true',
                    help='only process images without multipoint code signatures (scan targets)')
    args = ap.parse_args()
    corpus = args.corpus.resolve(); worker = args.worker.resolve()
    qual = json.load(open('/tmp/c167-corpus-qualify.json'))
    targets = []
    for r in qual['records']:
        has_sig = bool(r.get('code_signatures'))
        if args.non_signature_only and has_sig:
            continue
        if not args.non_signature_only and not has_sig:
            continue
        targets.append(r)
    if args.limit: targets = targets[:args.limit]
    records = []
    for r in targets:
        rel = r['path']
        path = corpus / rel
        data = path.read_bytes()
        prim = find_primitive(worker, path, data)
        if not prim:
            records.append({'path': rel, 'error': 'no_primitive'})
            print(f"{rel}: no_primitive", file=sys.stderr)
            continue
        rec = {'path': rel, 'entry': hex(prim['entry']), 'kind': prim.get('kind'),
               'params': {k: hex(v) for k,v in prim['params'].items()},
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
        print(f"{rel}: kind={rec['kind']} ok={rec['ok']} bad={rec['bad']} skip={rec['skip']}", file=sys.stderr)
    summary = {'images': len(records),
               'qualified': sum(1 for r in records if r.get('bad',0)==0 and r.get('ok',0)>0 and 'error' not in r),
               'total_ok': sum(r.get('ok',0) for r in records),
               'total_bad': sum(r.get('bad',0) for r in records),
               'total_skip': sum(r.get('skip',0) for r in records),
               'errors': sum(1 for r in records if 'error' in r),
               'kinds': {}}
    for r in records:
        k = r.get('kind') or ('error:' + r.get('error','?'))
        summary['kinds'][k] = summary['kinds'].get(k, 0) + 1
    report = {'schema': 1, 'summary': summary, 'records': records}
    out = json.dumps(report, indent=2, sort_keys=True) + '\n'
    if args.output: args.output.write_text(out)
    else: sys.stdout.write(out)
    return 0

if __name__ == '__main__':
    raise SystemExit(main())
