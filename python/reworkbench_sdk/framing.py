"""Four-byte big-endian bounded transport, matching the Rust codec."""
import struct
MAX_FRAME = 256 * 1024

def _exact(stream, count):
    data = bytearray()
    while len(data) < count:
        part = stream.read(count - len(data))
        if not part:
            raise EOFError("truncated frame")
        data.extend(part)
    return bytes(data)

def read_frame(stream):
    first = stream.read(1)
    if not first:
        return None
    count = struct.unpack(">I", first + _exact(stream, 3))[0]
    if count > MAX_FRAME:
        raise ValueError("frame exceeds limit")
    return _exact(stream, count)

def write_frame(stream, body):
    if len(body) > MAX_FRAME:
        raise ValueError("frame exceeds limit")
    data = memoryview(struct.pack(">I", len(body)) + body)
    while data:
        written = stream.write(data)
        if written is None or written <= 0:
            raise OSError("stream did not accept data")
        data = data[written:]
    stream.flush()
