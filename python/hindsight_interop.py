"""Hash-bound, image-relative function-name exchange for reversing tools.

Third-party packages are imported only by their respective adapters. Ghidra
requires PyGhidra (Python 3). Exchange names are analyst/tool assertions.
"""
import argparse
import hashlib
import json
import os
import pathlib
import re

MAX_BYTES = 16 * 1024 * 1024
MAX_FUNCTIONS = 20_000
U64 = 2**64 - 1
SCHEMA = "hindsight.functions.v1"


def bounded_json(path):
    with open(path, "rb") as stream:
        data = stream.read(MAX_BYTES + 1)
    if len(data) > MAX_BYTES:
        raise ValueError("JSON exceeds 16 MiB budget")
    return json.loads(data)


def address(value):
    if not isinstance(value, str) or not re.fullmatch(r"0x[0-9a-fA-F]{1,16}", value):
        raise ValueError("addresses must be hexadecimal strings within 64 bits")
    return int(value, 16)


def digest(value):
    if not isinstance(value, str) or not re.fullmatch(r"[0-9a-fA-F]{64}", value):
        raise ValueError("a valid source SHA-256 is required")
    return value.lower()


def text(value, limit=256):
    if not isinstance(value, str) or not value.strip() or len(value.encode("utf-8")) > limit:
        raise ValueError("invalid or over-budget name/provenance")
    if any(ord(ch) < 32 or ord(ch) == 127 for ch in value):
        raise ValueError("control characters are not allowed in names/provenance")
    return value


def validate(document):
    if not isinstance(document, dict) or document.get("schema") != SCHEMA or document.get("address_space") != "image-relative":
        raise ValueError("unsupported function exchange schema or address space")
    digest(document.get("source_sha256"))
    address(document.get("source_image_base"))
    text(document.get("producer"))
    functions = document.get("functions")
    if not isinstance(functions, list) or len(functions) > MAX_FUNCTIONS:
        raise ValueError("function list exceeds 20,000-record budget")
    seen = set()
    for row in functions:
        if not isinstance(row, dict):
            raise ValueError("invalid function record")
        offset = address(row.get("offset"))
        if offset + address(document["source_image_base"]) > U64:
            raise ValueError("source function address overflows")
        if offset in seen:
            raise ValueError("duplicate/conflicting function offset")
        seen.add(offset)
        text(row.get("name"))
        text(row.get("provenance"), 512)
    return document


def exchange(source_sha256, image_base, producer, rows):
    if type(image_base) is not int or not 0 <= image_base <= U64:
        raise ValueError("image base is outside 64-bit address space")
    functions = []
    for location, name, provenance in rows:
        if type(location) is not int or not image_base <= location <= U64:
            raise ValueError("function address precedes image base or exceeds 64 bits")
        functions.append({"offset": hex(location - image_base), "name": name, "provenance": provenance})
        if len(functions) > MAX_FUNCTIONS:
            raise ValueError("function list exceeds 20,000-record budget")
    functions.sort(key=lambda row: address(row["offset"]))
    return validate({"schema": SCHEMA, "address_space": "image-relative", "source_sha256": digest(source_sha256),
                     "source_image_base": hex(image_base), "producer": producer, "functions": functions})


def from_report(report, image_base):
    """Consume worker analysis JSON, including legacy and structural function rows."""
    if not isinstance(report, dict) or report.get("kind") != "analysis_result":
        raise ValueError("an analysis_result worker JSON report is required")
    value = report.get("functions")
    if not isinstance(value, str):
        raise ValueError("report has no function evidence")
    rows = []
    # Match only explicit, named function rows; never invent names from prose.
    pattern = re.compile(r"^(0x[0-9a-fA-F]+) name=(.*?) source=(.+)$")
    for line in value.splitlines():
        match = pattern.fullmatch(line)
        if match:
            rows.append((address(match[1]), match[2], match[3]))
    if not rows:
        # ARM64 rows carry explicit start/end/size and boundary provenance.
        pattern = re.compile(r"^(0x[0-9a-fA-F]+)\s+0x[0-9a-fA-F]+\s+\d+ bytes\s+(.*?)\s+source=(.+)$")
        for line in value.splitlines():
            match = pattern.fullmatch(line)
            if match and match[2] != "<discovered>":
                rows.append((address(match[1]), match[2], match[3]))
    if not rows:
        raise ValueError("report has no supported named function records")
    return exchange(report.get("source_sha256"), image_base, "HindSight", rows)


def save(document, path):
    output = json.dumps(validate(document), ensure_ascii=False, indent=2) + "\n"
    if len(output.encode("utf-8")) > MAX_BYTES:
        raise ValueError("exchange exceeds 16 MiB budget")
    with open(path, "x", encoding="utf-8") as stream:
        stream.write(output)


def artifact_digest(path):
    result = hashlib.sha256()
    with open(path, "rb") as stream:
        before = os.fstat(stream.fileno())
        if before.st_size > 8 * 1024**3:
            raise ValueError("artifact exceeds 8 GiB budget")
        while True:
            block = stream.read(1024 * 1024)
            if not block:
                break
            result.update(block)
        after = os.fstat(stream.fileno())
        if (before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (after.st_size, after.st_mtime_ns, after.st_ctime_ns):
            raise ValueError("artifact changed during hashing")
    return result.hexdigest()


def catalog(document, artifact, image_base):
    validate(document)
    if artifact_digest(artifact) != digest(document["source_sha256"]):
        raise ValueError("artifact SHA-256 does not match function exchange")
    if type(image_base) is not int or not 0 <= image_base <= U64:
        raise ValueError("invalid target image base")
    lines = ["# HindSight function exchange; names are external assertions",
             "# source_sha256=" + digest(document["source_sha256"]),
             "# producer=" + document["producer"]]
    for row in document["functions"]:
        location = image_base + address(row["offset"])
        if location > U64:
            raise ValueError("rebased function address overflows")
        lines.append("# provenance=" + row["provenance"])
        lines.append(f"{location:#x} {row['name']}")
    return "\n".join(lines) + "\n"


def export_adapter(adapter, path):
    document = exchange(adapter.source_sha256(), adapter.image_base(), adapter.producer, adapter.functions())
    save(document, path)
    return len(document["functions"])


def import_adapter(adapter, path, *, dry_run=True, replace=False):
    """Plan first; mutate only existing function names after identity validation."""
    document = validate(bounded_json(path))
    if digest(adapter.source_sha256()) != digest(document["source_sha256"]):
        raise ValueError("tool input SHA-256 does not match function exchange")
    base = adapter.image_base()
    if type(base) is not int or not 0 <= base <= U64:
        raise ValueError("invalid tool image base")
    plan = []
    for row in document["functions"]:
        location = base + address(row["offset"])
        if location > U64:
            raise ValueError("rebased function address overflows")
        previous = adapter.name_at(location)
        status = "missing" if previous is None else "same" if previous == row["name"] else "rename" if replace else "conflict"
        plan.append({"address": hex(location), "old_name": previous, "name": row["name"], "status": status,
                     "provenance": row["provenance"], "producer": document["producer"]})
    # Finish validation and planning before beginning any database mutations.
    if not dry_run:
        with adapter.transaction():
            for row in plan:
                if row["status"] == "rename":
                    if adapter.rename(address(row["address"]), row["name"]) is False:
                        row["status"] = "rejected"
                    else:
                        row["status"] = "applied"
    return plan


class Ghidra:
    producer = "Ghidra"

    def __init__(self, program):
        self.program = program

    def source_sha256(self):
        return str(self.program.getExecutableSHA256())

    def image_base(self):
        return int(self.program.getImageBase().getOffset()) & U64

    def functions(self):
        space = self.program.getImageBase().getAddressSpace()
        for function in self.program.getFunctionManager().getFunctions(True):
            location = function.getEntryPoint()
            if not function.isExternal() and location.getAddressSpace() == space:
                yield int(location.getOffset()) & U64, str(function.getName()), "Ghidra function name; " + str(function.getSymbol().getSource())

    def _function(self, location):
        # String parsing avoids signed Java-long overflow for high kernel addresses.
        space = self.program.getImageBase().getAddressSpace()
        return self.program.getFunctionManager().getFunctionAt(space.getAddress(hex(location)))

    def name_at(self, location):
        function = self._function(location)
        return None if function is None else str(function.getName())

    def rename(self, location, name):
        from ghidra.program.model.symbol import SourceType
        self._function(location).setName(name, SourceType.USER_DEFINED)

    def transaction(self):
        from contextlib import contextmanager

        @contextmanager
        def scope():
            identifier = self.program.startTransaction("Import HindSight function names")
            success = False
            try:
                yield
                success = True
            finally:
                self.program.endTransaction(identifier, success)
        return scope()


class IDA:
    producer = "IDA"

    def source_sha256(self):
        import ida_nalt
        value = ida_nalt.retrieve_input_file_sha256()
        return bytes(value).hex() if value else ""

    def image_base(self):
        import ida_nalt
        return int(ida_nalt.get_imagebase())

    def functions(self):
        import ida_funcs
        import idautils
        for location in idautils.Functions():
            yield int(location), ida_funcs.get_func_name(location), "IDA function name; tool/analyst assertion"

    def name_at(self, location):
        import ida_funcs
        function = ida_funcs.get_func(location)
        return ida_funcs.get_func_name(location) if function and function.start_ea == location else None

    def rename(self, location, name):
        import ida_name
        return bool(ida_name.set_name(location, name, ida_name.SN_CHECK))

    def transaction(self):
        # IDA has no portable rollback API; rejected names are returned individually.
        from contextlib import nullcontext
        return nullcontext()


class BinaryNinja:
    producer = "Binary Ninja"

    def __init__(self, view, artifact, image_base):
        self.view = view
        self.artifact = artifact
        self.base = image_base

    def source_sha256(self):
        # Hash original captured bytes explicitly, never a .bndb or loaded memory.
        raw = self.view.file.raw
        if raw is None:
            raise ValueError("Binary Ninja raw input view is unavailable")
        if raw.length > 8 * 1024**3:
            raise ValueError("Binary Ninja raw input exceeds 8 GiB budget")
        result = hashlib.sha256()
        for offset in range(0, raw.length, 1024 * 1024):
            count = min(1024 * 1024, raw.length - offset)
            block = raw.read(offset, count)
            if len(block) != count:
                raise ValueError("short read from Binary Ninja raw input")
            result.update(block)
        expected = artifact_digest(self.artifact)
        if result.hexdigest() != expected:
            raise ValueError("Binary Ninja raw bytes do not match supplied artifact")
        return expected

    def image_base(self):
        return self.base

    def functions(self):
        seen = set()
        for function in self.view.functions:
            if function.start in seen:
                raise ValueError("multiple platform functions share an entry; select a single-platform view")
            seen.add(function.start)
            yield int(function.start), function.name, "Binary Ninja function name; tool/analyst assertion"

    def _function(self, location):
        functions = self.view.get_functions_at(location)
        if len(functions) > 1:
            raise ValueError("ambiguous multi-platform function entry")
        return functions[0] if functions else None

    def name_at(self, location):
        function = self._function(location)
        return function.name if function else None

    def rename(self, location, name):
        self._function(location).name = name

    def transaction(self):
        return self.view.undoable_transaction()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    report = commands.add_parser("from-report", help="export named function evidence from worker JSON")
    report.add_argument("report", type=pathlib.Path)
    report.add_argument("--image-base", type=address, required=True)
    report.add_argument("--output", type=pathlib.Path, required=True)
    names = commands.add_parser("to-catalog", help="verify artifact and convert exchange to HindSight names")
    names.add_argument("exchange", type=pathlib.Path)
    names.add_argument("--artifact", type=pathlib.Path, required=True)
    names.add_argument("--image-base", type=address, required=True)
    names.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args(argv)
    try:
        if args.command == "from-report":
            save(from_report(bounded_json(args.report), args.image_base), args.output)
        else:
            output = catalog(bounded_json(args.exchange), args.artifact, args.image_base)
            with args.output.open("x", encoding="utf-8") as stream:
                stream.write(output)
    except (OSError, ValueError, TypeError) as error:
        parser.exit(1, str(error) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
