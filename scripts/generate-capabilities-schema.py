#!/usr/bin/env python3
from __future__ import annotations
import argparse, codecs, json, pathlib, re, sys
ROOT = pathlib.Path(__file__).resolve().parents[1]
BRIDGE = ROOT / 'rust/crates/bridge/src/lib.rs'
SCHEMA = ROOT / 'schemas/capabilities.schema.json'

def bridge_capabilities() -> dict:
    text = BRIDGE.read_text()
    match = re.search(r'static CAPABILITIES: &\[u8\] = b"(.*?)\\0";', text, re.S)
    if not match:
        raise SystemExit('Unable to locate CAPABILITIES byte string')
    raw = codecs.decode(match.group(1), 'unicode_escape')
    value = json.loads(raw)
    if not isinstance(value, dict):
        raise SystemExit('CAPABILITIES must decode to a JSON object')
    return value

def generated_schema(value: dict) -> dict:
    properties = {}
    for key, item in value.items():
        if key == 'abi':
            properties[key] = {'const': item}
        elif key == 'release_channel':
            properties[key] = {'enum': ['beta', 'rc', 'stable']}
        elif isinstance(item, bool):
            properties[key] = {'const': item}
        elif isinstance(item, int):
            properties[key] = {'type': 'integer'}
        elif isinstance(item, str):
            properties[key] = {'type': 'string'}
        else:
            raise SystemExit(f'Unsupported capability value type for {key}: {type(item).__name__}')
    return {
        '$schema': 'https://json-schema.org/draft/2020-12/schema',
        'title': 'HindSight capability response',
        'type': 'object',
        'additionalProperties': False,
        'required': list(value.keys()),
        'properties': properties,
    }

def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    schema = generated_schema(bridge_capabilities())
    rendered = json.dumps(schema, indent=2) + '\n'
    if args.check:
        if not SCHEMA.is_file() or SCHEMA.read_text() != rendered:
            print('capabilities.schema.json is stale; run scripts/generate-capabilities-schema.py', file=sys.stderr)
            return 2
        print('Capabilities schema matches bridge response.')
        return 0
    SCHEMA.write_text(rendered)
    print(f'Generated {SCHEMA.relative_to(ROOT)} with {len(schema["required"])} fields.')
    return 0
if __name__ == '__main__':
    raise SystemExit(main())
