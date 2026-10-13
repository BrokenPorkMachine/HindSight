import importlib.util
from pathlib import Path
import struct
import tempfile
import unittest

MODULE = Path(__file__).resolve().parents[2] / 'scripts' / 'macho_uuid.py'
spec = importlib.util.spec_from_file_location('macho_uuid', MODULE)
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)


def macho(uuid=b'\x01' * 16, cpu=0x0100000c, subtype=0, endian='<'):
    magic = b'\xcf\xfa\xed\xfe' if endian == '<' else b'\xfe\xed\xfa\xcf'
    command = struct.pack(endian + 'II', 0x1b, 24) + uuid
    return magic + struct.pack(endian + 'IIIIIII', cpu, subtype, 2, 1, len(command), 0, 0) + command


class UUIDTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)

    def put(self, name, data):
        path = Path(self.temp.name) / name
        path.write_bytes(data)
        return path

    def test_matching_and_mismatch(self):
        a = self.put('main', macho())
        b = self.put('debug', macho())
        self.assertTrue(mod.match(a, b)[0]['matched'])
        b.write_bytes(macho(b'\x02' * 16))
        self.assertFalse(mod.match(a, b)[0]['matched'])

    def test_big_endian(self):
        self.assertEqual(mod.inspect(self.put('big', macho(endian='>')))[0]['cpu'], 'arm64')

    def test_reject_duplicate_uuid(self):
        data = bytearray(macho())
        data[16:20] = struct.pack('<I', 2)
        data[20:24] = struct.pack('<I', 48)
        data += data[32:56]
        with self.assertRaises(mod.InvalidMachO):
            mod.inspect(self.put('dupe', data))

    def test_fat_and_overlap(self):
        a, b = macho(), macho(cpu=0x01000007)
        header = b'\xca\xfe\xba\xbe' + struct.pack('>I', 2)
        hdr = struct.pack('>IIIII', 0x0100000c, 0, 48, len(a), 0) + struct.pack('>IIIII', 0x01000007, 0, 48 + len(a), len(b), 0)
        data = header + hdr + a + b
        self.assertEqual(len(mod.inspect(self.put('fat', data))), 2)
        broken = bytearray(data)
        struct.pack_into('>I', broken, 8 + 20 + 8, 48)
        with self.assertRaises(mod.InvalidMachO):
            mod.inspect(self.put('overlap', broken))

    def test_reject_truncation(self):
        with self.assertRaises(mod.InvalidMachO):
            mod.inspect(self.put('cut', macho()[:-1]))


if __name__ == '__main__':
    unittest.main()
