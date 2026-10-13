#!/usr/bin/env python3
"""Bounded, dependency-free Mach-O UUID inspector for dSYM pairing.

Accepts thin Mach-O and FAT32/FAT64 containers, both endian variants. Does not
trust filename, bundle metadata or symbols as a replacement for LC_UUID.
"""
import argparse
import json
from pathlib import Path
import struct
import sys

MAX_SLICES = 128
MAX_COMMANDS = 8192
MAX_COMMAND_BYTES = 64 * 1024 * 1024
CPU_NAMES = {7: 'i386', 0x01000007: 'x86_64', 12: 'arm', 0x0100000c: 'arm64'}
THIN = {b'\xce\xfa\xed\xfe': ('<', False), b'\xfe\xed\xfa\xce': ('>', False),
        b'\xcf\xfa\xed\xfe': ('<', True), b'\xfe\xed\xfa\xcf': ('>', True)}
FAT = {b'\xca\xfe\xba\xbe': ('>', False), b'\xbe\xba\xfe\xca': ('<', False),
       b'\xca\xfe\xba\xbf': ('>', True), b'\xbf\xba\xfe\xca': ('<', True)}


class InvalidMachO(ValueError):
    pass


def _read(file, offset, count, total):
    if offset < 0 or count < 0 or offset > total or count > total - offset:
        raise InvalidMachO('file extent outside bounds')
    file.seek(offset)
    data = file.read(count)
    if len(data) != count:
        raise InvalidMachO('truncated input')
    return data


def _thin(file, start, length, total):
    head = _read(file, start, min(length, 32), total)
    if len(head) < 28 or head[:4] not in THIN:
        raise InvalidMachO('invalid thin Mach-O header')
    endian, is64 = THIN[head[:4]]
    minimum = 32 if is64 else 28
    if len(head) < minimum:
        raise InvalidMachO('truncated Mach-O header')
    cpu, subtype, _, ncmds, sizeofcmds = struct.unpack_from(endian + 'IIIII', head, 4)
    if ncmds > MAX_COMMANDS or sizeofcmds > MAX_COMMAND_BYTES or sizeofcmds > length - minimum:
        raise InvalidMachO('invalid load command budget')
    commands = _read(file, start + minimum, sizeofcmds, total)
    pos, uuids = 0, []
    for _ in range(ncmds):
        if pos + 8 > len(commands):
            raise InvalidMachO('truncated load command')
        cmd, size = struct.unpack_from(endian + 'II', commands, pos)
        if size < 8 or pos + size > len(commands):
            raise InvalidMachO('invalid load command extent')
        if cmd == 0x1b:
            if size != 24:
                raise InvalidMachO('invalid LC_UUID length')
            value = commands[pos + 8:pos + 24].hex()
            uuids.append('-'.join((value[:8], value[8:12], value[12:16], value[16:20], value[20:])))
        pos += size
    if pos != sizeofcmds or len(uuids) > 1:
        raise InvalidMachO('ambiguous load commands or duplicate LC_UUID')
    return {'cpu': CPU_NAMES.get(cpu, f'cpu:{cpu:#x}'), 'cpu_type': cpu,
            'cpu_subtype': subtype, 'uuid': uuids[0] if uuids else None,
            'offset': start, 'size': length}


def inspect(path):
    with open(path, 'rb') as file:
        total = file.seek(0, 2)
        if total < 4:
            raise InvalidMachO('input too small')
        magic = _read(file, 0, 4, total)
        if magic in THIN:
            return [_thin(file, 0, total, total)]
        if magic not in FAT:
            raise InvalidMachO('unsupported Mach-O magic')
        endian, wide = FAT[magic]
        header = _read(file, 0, 8, total)
        count = struct.unpack_from(endian + 'I', header, 4)[0]
        if count < 1 or count > MAX_SLICES:
            raise InvalidMachO('invalid FAT architecture count')
        stride = 32 if wide else 20
        entries = _read(file, 8, count * stride, total)
        ranges, result, identities = [], [], set()
        for i in range(count):
            pos = i * stride
            cpu, subtype = struct.unpack_from(endian + 'II', entries, pos)
            off, length = struct.unpack_from(endian + ('QQ' if wide else 'II'), entries, pos + 8)
            if length == 0 or off < 8 + count * stride or off > total or length > total - off:
                raise InvalidMachO('invalid FAT slice extent')
            if any(off < b and a < off + length for a, b in ranges):
                raise InvalidMachO('overlapping FAT slices')
            ranges.append((off, off + length))
            entry = _thin(file, off, length, total)
            if (cpu, subtype) != (entry['cpu_type'], entry['cpu_subtype']):
                raise InvalidMachO('FAT architecture and Mach-O header disagree')
            identity = (cpu, subtype)
            if identity in identities:
                raise InvalidMachO('duplicate FAT architecture')
            identities.add(identity)
            result.append(entry)
        return result


def match(binary, dsym):
    main = inspect(binary)
    debug = inspect(dsym)
    matches = []
    for candidate in main:
        other = next((s for s in debug if (s['cpu_type'], s['cpu_subtype']) ==
                      (candidate['cpu_type'], candidate['cpu_subtype'])), None)
        matches.append({'architecture': candidate['cpu'], 'cpu_subtype': candidate['cpu_subtype'],
                        'binary_uuid': candidate['uuid'],
                        'dsym_uuid': other['uuid'] if other else None,
                        'matched': bool(other and candidate['uuid'] and candidate['uuid'] == other['uuid'])})
    return matches


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path)
    parser.add_argument('--dsym', type=Path, help='dSYM DWARF Mach-O file to compare')
    args = parser.parse_args(argv)
    try:
        output = match(args.binary, args.dsym) if args.dsym else inspect(args.binary)
    except (OSError, InvalidMachO) as error:
        print(f'Mach-O inspection failed: {error}', file=sys.stderr)
        return 2
    print(json.dumps(output, indent=2))
    return 0 if not args.dsym or all(item['matched'] for item in output) else 1


if __name__ == '__main__':
    raise SystemExit(main())
