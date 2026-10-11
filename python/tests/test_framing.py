import io
import struct
import unittest
from reworkbench_sdk.framing import read_frame, write_frame, MAX_FRAME

class FramingTests(unittest.TestCase):
    def test_round_trip(self):
        out = io.BytesIO()
        for body in [b"", b"hello", bytes(range(256))]: write_frame(out, body)
        out.seek(0)
        for body in [b"", b"hello", bytes(range(256))]: self.assertEqual(read_frame(out), body)
        self.assertIsNone(read_frame(out))
    def test_wire_bytes(self):
        out = io.BytesIO(); write_frame(out, b"abc")
        self.assertEqual(out.getvalue(), b"\x00\x00\x00\x03abc")
    def test_truncated_header(self):
        with self.assertRaises(EOFError): read_frame(io.BytesIO(b"\x00\x00"))
    def test_truncated_body(self):
        with self.assertRaises(EOFError): read_frame(io.BytesIO(b"\x00\x00\x00\x02a"))
    def test_oversize(self):
        with self.assertRaises(ValueError): read_frame(io.BytesIO(struct.pack(">I", MAX_FRAME + 1)))
    def test_short_io(self):
        class ShortReader(io.BytesIO):
            def read(self, n=-1): return super().read(min(n, 1))
        self.assertEqual(read_frame(ShortReader(b"\x00\x00\x00\x03abc")), b"abc")
        class ShortWriter(io.BytesIO):
            def write(self, b): return super().write(b[:1])
        stream=ShortWriter(); write_frame(stream,b"abc")
        self.assertEqual(stream.getvalue(),b"\x00\x00\x00\x03abc")

if __name__ == "__main__": unittest.main()
