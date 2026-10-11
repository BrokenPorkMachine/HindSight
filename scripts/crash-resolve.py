#!/usr/bin/env python3
"""Bounded crash-address resolver for HindSight unified-symbol text evidence.

The tool consumes .crash Binary Images plus --symbols (71-field analysis report JSON),
or --symbol-text (the symbol_resolver field saved verbatim). The static image base
must be supplied explicitly, and the target image UUID must match. No guesses.
"""
import argparse
import bisect
import json
import pathlib
import re
import sys

RANGE = re.compile(r"^range start=(0x[0-9a-fA-F]+) end=(0x[0-9a-fA-F]+) size=\d+ symbol=(.*?) source=(\S+)(?: aliases=.*)?$")
IMAGE = re.compile(r"^\s*(0x[0-9a-fA-F]+)\s*-\s*(0x[0-9a-fA-F]+)\s+(.+?)\s+(?:\([^)]*\)\s+)?<([0-9a-fA-F-]{32,36})>\s+(.*)$")
HEX = re.compile(r"0x[0-9a-fA-F]+")
FRAME = re.compile(r"^(\s*\d+\s+)(\S+)(\s+)(0x[0-9a-fA-F]+)(.*)$")
MAX_INPUT = 16 * 1024 * 1024
MAX_ROWS = 4096


def normalized_uuid(value):
    return re.sub(r"[^0-9a-f]", "", value.lower())


def read_bounded(path):
    with open(path, 'rb') as f:
        data = f.read(MAX_INPUT + 1)
    if len(data) > MAX_INPUT:
        raise ValueError('input exceeds 16 MiB limit')
    return data.decode('utf-8', 'replace')


def ranges_from_text(value):
    rows = []
    for line in value.splitlines():
        m = RANGE.match(line)
        if not m:
            continue
        start, end = int(m[1], 16), int(m[2], 16)
        if not (0 <= start < end <= 2**64):
            continue
        rows.append((start, end, m[3], m[4]))
        if len(rows) > MAX_ROWS:
            raise ValueError('symbol-range limit exceeded')
    rows.sort(key=lambda r: (r[0], r[1]))
    if any(a[1] > b[0] for a, b in zip(rows, rows[1:])):
        raise ValueError('overlapping symbol ranges cannot be safely resolved')
    return rows


def crash_images(text):
    if '\nBinary Images:' not in text:
        raise ValueError('no Binary Images section (.crash text required)')
    images = []
    for line in text.split('Binary Images:', 1)[1].splitlines():
        m = IMAGE.match(line)
        if m:
            start, end = int(m[1], 16), int(m[2], 16)
            if end >= start:
                images.append((start, end + 1, normalized_uuid(m[4]), m[3].strip()))
        if len(images) > 4096:
            raise ValueError('too many binary images')
    return images


def resolve(crash, symbols, expected_uuid, static_base):
    uid = normalized_uuid(expected_uuid)
    if len(uid) != 32:
        raise ValueError('expected UUID must contain 32 hex digits')
    images = [x for x in crash_images(crash) if x[2] == uid]
    if len(images) != 1:
        raise ValueError(f'expected exactly one matching UUID image, found {len(images)}')
    runtime_start, runtime_end, _, image_name = images[0]
    if static_base < 0 or static_base >= 2**64:
        raise ValueError('invalid static image base')
    rows = ranges_from_text(symbols)
    starts = [r[0] for r in rows]
    out = []
    for line in crash.splitlines():
        m = FRAME.match(line)
        if m:
            address = int(m[4], 16)
            if runtime_start <= address < runtime_end:
                static = static_base + (address - runtime_start)
                if static < 2**64:
                    i = bisect.bisect_right(starts, static) - 1
                    if i >= 0 and static < rows[i][1]:
                        row = rows[i]
                        line += f' [VG {row[2]}+0x{static-row[0]:x}; {row[3]}; UUID-verified]'
                    else:
                        line += ' [VG unresolved: no bounded symbol range]'
        out.append(line)
    return '\n'.join(out) + '\n'


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('crash', help='text .crash report with Binary Images section')
    g = p.add_mutually_exclusive_group(required=True)
    g.add_argument('--symbols', help='HindSight JSON analysis report')
    g.add_argument('--symbol-text', help='raw symbol_resolver field text')
    p.add_argument('--uuid', required=True, help='UUID of analyzed binary (from LC_UUID)')
    p.add_argument('--static-base', required=True, type=lambda x:int(x,0), help='static Mach-O image base, e.g. 0x100000000')
    a = p.parse_args()
    try:
        if a.symbols:
            doc = json.loads(read_bounded(a.symbols))
            source = doc.get('symbol_resolver')
            if not isinstance(source, str):
                raise ValueError('analysis JSON has no symbol_resolver text')
        else:
            source = read_bounded(a.symbol_text)
        sys.stdout.write(resolve(read_bounded(a.crash),source,a.uuid,a.static_base))
    except (OSError, ValueError, json.JSONDecodeError) as ex:
        p.exit(2, f'error: {ex}\n')

if __name__ == '__main__':
    main()
