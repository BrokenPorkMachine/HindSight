#!/usr/bin/env python3
"""One-shot Linux static analysis via Docker; stdout is JSON or a worker frame.

This is a local administrative tool, not a remotely callable Docker gateway.
Never give untrusted clients Docker arguments, image selection or host paths.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import stat
import struct
import subprocess
import sys
import time
import uuid

IMAGE = "hindsight-worker:local"
MAX_FRAME = 32 * 1024 * 1024
MAX_SOURCE = 8 * 1024 * 1024 * 1024


def unsigned(text):
    if not re.fullmatch(r"(?:0x[0-9a-fA-F]+|[0-9]+)", text):
        raise argparse.ArgumentTypeError("expected an unsigned decimal or 0x hexadecimal integer")
    value = int(text, 16 if text.startswith("0x") else 10)
    if value > (1 << 64) - 1:
        raise argparse.ArgumentTypeError("integer exceeds uint64")
    return value


def source_identity(path):
    # Reject a symlink leaf; pin parent symlinks to their resolved path. Worker
    # verifies the digest again after Docker mounts it, catching path races.
    if path.is_symlink():
        raise ValueError("input must not be a symbolic link")
    resolved = path.resolve(strict=True)
    if "," in str(resolved):
        raise ValueError("Docker mount source paths cannot contain commas")
    fd = os.open(resolved, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    before = os.fstat(fd)
    if not stat.S_ISREG(before.st_mode) or before.st_size > MAX_SOURCE:
        os.close(fd)
        raise ValueError("input must be a regular file no larger than 8 GiB")
    with os.fdopen(fd, "rb") as stream:
        digest = hashlib.sha256()
        total = 0
        while chunk := stream.read(1024 * 1024):
            total += len(chunk)
            if total > before.st_size:
                raise ValueError("input grew while hashing")
            digest.update(chunk)
        after = os.fstat(stream.fileno())
        fingerprint = lambda s: (s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns, s.st_ctime_ns)
        if total != before.st_size or fingerprint(before) != fingerprint(after):
            raise ValueError("input changed while hashing")
    return resolved, digest.hexdigest(), str(total)


def command(args, name):
    if not re.fullmatch(r"[a-zA-Z0-9][a-zA-Z0-9._/@:-]{0,255}", args.image):
        raise ValueError("invalid image reference; build or pull a trusted worker image first")
    # Use the invoking non-root user's identity so owner-only captured files
    # remain readable on Linux. A root invoker never makes the worker root.
    uid, gid = (os.getuid(), os.getgid()) if os.getuid() else (65532, 65532)
    docker = ["docker", "run", "--rm", "--name", name, "--pull=never",
              "--network=none", "--read-only", f"--user={uid}:{gid}",
              "--cap-drop=ALL", "--security-opt=no-new-privileges=true",
              "--cpus=2", "--memory=1g", "--memory-swap=1g", "--pids-limit=64",
              "--ulimit=nofile=256:256", "--ulimit=core=0:0", "--log-driver=none"]
    if args.operation == "health":
        return docker + [args.image, "--self-test"], None
    path, digest, size = source_identity(args.path)
    docker += ["--mount", f"type=bind,source={path},target=/input.bin,readonly"]
    request = str(uuid.uuid4())
    identity = ["/input.bin", digest, size]
    options = [args.format, args.architecture, str(args.load_address), str(args.file_offset)]
    if args.operation == "analyze":
        worker = ["--analyze-image", *identity, request, *options]
    elif args.operation == "page":
        worker = ["--page-image", *identity, str(args.start_word), request, *options]
    else:
        if args.start is None or args.end is None or args.end <= args.start:
            raise ValueError("reconstruct/interpret require --start and --end with end > start")
        if options != ["auto", "auto", "0", "0"]:
            raise ValueError("reconstruct/interpret currently accept only automatic thin AArch64 Mach-O input")
        worker = ["--" + args.operation, *identity, hex(args.start), hex(args.end)]
        if args.operation == "interpret":
            if len(args.inputs) != 8:
                raise ValueError("interpret requires exactly eight comma-separated uint64 inputs")
            worker += [",".join(hex(value) for value in args.inputs)]
        worker += [request]
    return docker + [args.image, *worker], digest


def execute(argv, name, timeout):
    process = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE)
    selector = selectors.DefaultSelector()
    output, errors = bytearray(), bytearray()
    selector.register(process.stdout, selectors.EVENT_READ, output)
    selector.register(process.stderr, selectors.EVENT_READ, errors)
    deadline = time.monotonic() + timeout
    completed = False
    try:
        while selector.get_map():
            if time.monotonic() >= deadline:
                raise TimeoutError("Linux worker exceeded its wall-clock limit")
            for key, _ in selector.select(min(0.25, max(0, deadline - time.monotonic()))):
                chunk = os.read(key.fileobj.fileno(), 65536)
                if not chunk:
                    selector.unregister(key.fileobj)
                    continue
                target = key.data
                limit = MAX_FRAME + 4 if target is output else 256 * 1024
                if len(target) + len(chunk) > limit:
                    raise ValueError("Linux worker exceeded its output budget")
                target.extend(chunk)
        code = process.wait(timeout=max(0.01, deadline-time.monotonic()))
        completed = True
        if errors:
            sys.stderr.buffer.write(errors)
        if code:
            raise RuntimeError(f"Docker worker exited with status {code}")
        return bytes(output)
    finally:
        selector.close()
        process.stdout.close()
        process.stderr.close()
        if not completed:
            process.kill()
            process.wait()
            # Only this invocation's unpredictable name; never broad cleanup.
            try:
                cleanup = subprocess.run(["docker", "rm", "--force", name],
                                         stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, timeout=15)
                if cleanup.returncode:
                    print(f"Check container cleanup manually: {name}", file=sys.stderr)
            except (OSError, subprocess.TimeoutExpired):
                print(f"Check container cleanup manually: {name}", file=sys.stderr)


def decode_frame(raw, digest=None):
    if len(raw) < 4:
        raise ValueError("worker reply is missing its length prefix")
    length = struct.unpack_from(">I", raw)[0]
    if length > MAX_FRAME or len(raw) != length + 4:
        raise ValueError("worker reply has an invalid frame length")
    document = json.loads(raw[4:])
    if not isinstance(document, dict):
        raise ValueError("worker reply must be an object")
    if digest and document.get("kind") != "error" and document.get("source_sha256") != digest:
        raise ValueError("worker reply does not match the selected input")
    return document


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=["health", "analyze", "page", "reconstruct", "interpret"])
    parser.add_argument("path", nargs="?", type=Path)
    parser.add_argument("--image", default=IMAGE, help="trusted prebuilt image (never auto-pulled)")
    parser.add_argument("--format", choices=["auto", "macho", "elf", "pe", "coff", "pdb", "raw"], default="auto")
    parser.add_argument("--architecture", choices=["auto", "aarch64", "x86_64", "i386", "arm", "thumb"], default="auto")
    parser.add_argument("--load-address", type=unsigned, default=0)
    parser.add_argument("--file-offset", type=unsigned, default=0)
    parser.add_argument("--start-word", type=unsigned, default=0)
    parser.add_argument("--start", type=unsigned)
    parser.add_argument("--end", type=unsigned)
    parser.add_argument("--inputs", type=lambda s: [unsigned(v) for v in s.split(",")], default=[0]*8)
    parser.add_argument("--timeout", type=int, default=180)
    parser.add_argument("--framed", action="store_true", help="preserve the binary worker frame on stdout")
    args = parser.parse_args()
    if (args.operation == "health") != (args.path is None):
        parser.error("health takes no input path; other operations require one")
    if not 1 <= args.timeout <= 3600:
        parser.error("timeout must be 1..3600 seconds")
    try:
        name = "hindsight-worker-" + uuid.uuid4().hex
        argv, digest = command(args, name)
        raw = execute(argv, name, args.timeout)
        document = decode_frame(raw, digest)
        if args.framed:
            sys.stdout.buffer.write(raw)
        else:
            print(json.dumps(document, ensure_ascii=True, indent=2))
        return 1 if document.get("kind") == "error" else 0
    except (OSError, ValueError, RuntimeError, TimeoutError, subprocess.TimeoutExpired) as error:
        print(f"HindSight Docker worker: {error}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        print("HindSight Docker worker cancelled", file=sys.stderr)
        return 130


if __name__ == "__main__":
    sys.exit(main())
