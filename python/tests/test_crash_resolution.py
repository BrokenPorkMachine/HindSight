import importlib.util
import pathlib
import unittest

P = pathlib.Path(__file__).resolve().parents[2] / 'scripts' / 'crash-resolve.py'
spec = importlib.util.spec_from_file_location('vg_crash_resolve', P)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
UUID = 'AABBCCDD-1122-3344-5566-77889900AABB'
CRASH = f"""Process: Example
Thread 0 Crashed:
0   Example 0x0000000100001012 some
1   Example 0x0000000100002010 other
Binary Images:
       0x100000000 - 0x10000ffff Example arm64 <{UUID}> /tmp/Example
"""
SYMBOLS = 'range start=0x0000000100001000 end=0x0000000100001040 size=64 symbol=_hello source=nlist\n'
class TestCrashResolution(unittest.TestCase):
    def test_resolves_verified_image(self):
        s = m.resolve(CRASH, SYMBOLS, UUID, 0x100000000)
        self.assertIn('_hello+0x12; nlist; UUID-verified', s)
        self.assertIn('unresolved: no bounded symbol range', s)
    def test_uuid_mismatch_fails_closed(self):
        with self.assertRaisesRegex(ValueError, 'matching UUID'):
            m.resolve(CRASH, SYMBOLS, '0'*32, 0x100000000)
    def test_overlap_rejected(self):
        with self.assertRaisesRegex(ValueError, 'overlapping'):
            m.ranges_from_text(SYMBOLS+'range start=0x100001020 end=0x100001050 size=48 symbol=_bad source=nlist\n')
    def test_no_binary_images_fails(self):
        with self.assertRaises(ValueError):
            m.resolve('broken', SYMBOLS, UUID, 0x100000000)
    def test_slide(self):
        shifted = CRASH.replace('0x100000000','0x110000000').replace('0x10000ffff','0x11000ffff').replace('0x0000000100001012','0x0000000110001012').replace('0x0000000100002010','0x0000000110002010')
        self.assertIn('_hello+0x12',m.resolve(shifted,SYMBOLS,UUID,0x100000000))
