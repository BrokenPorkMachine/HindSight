import pathlib
import unittest


def sign_extend(value: int, bits: int) -> int:
    sign = 1 << (bits - 1)
    return (value ^ sign) - sign


def resolve(site: int, rel: int) -> int:
    return site + rel


def adr_target(word: int, pc: int) -> int:
    imm = (((word >> 5) & 0x7FFFF) << 2) | ((word >> 29) & 0x3)
    return resolve(pc, sign_extend(imm, 21))


def adrp_page(word: int, pc: int) -> int:
    imm = (((word >> 5) & 0x7FFFF) << 2) | ((word >> 29) & 0x3)
    return resolve(pc & ~0xFFF, sign_extend(imm, 21) << 12)


def add_imm(word: int) -> int:
    shift = 12 if ((word >> 22) & 1) else 0
    return ((word >> 10) & 0xFFF) << shift


def literal_target(word: int, pc: int) -> int:
    rel = sign_extend(((word >> 5) & 0x7FFFF) << 2, 21)
    return resolve(pc, rel)


class AnalysisSynthesisImmediateTests(unittest.TestCase):
    """Known encodings independently assembled with Clang/LLVM AArch64.

    These vectors validate the immediate arithmetic used by the Rust synthesis
    pass even on portable hosts where the pinned Cargo toolchain is absent.
    """

    def test_adr_known_vector(self):
        # 0x10268: adr x0, 0x10280
        self.assertEqual(0x100000C0 & 0x9F000000, 0x10000000)
        self.assertEqual(adr_target(0x100000C0, 0x10268), 0x10280)

    def test_adrp_add_known_vector(self):
        # --no-relax linked sequence:
        # 0x1026c: adrp x1, 0x0
        # 0x10270: add  x1, x1, #0x258
        adrp = 0x90FFFF81
        add = 0x91096021
        self.assertEqual(adrp & 0x9F000000, 0x90000000)
        self.assertEqual(add & 0x7F000000, 0x11000000)
        self.assertEqual((add >> 31) & 1, 1)
        self.assertEqual(adrp_page(adrp, 0x1026C), 0x0)
        self.assertEqual(add_imm(add), 0x258)
        self.assertEqual(adrp_page(adrp, 0x1026C) + add_imm(add), 0x258)

    def test_literal_known_vector(self):
        # 0x10274: ldr x2, 0x10288
        word = 0x580000A2
        self.assertEqual(word & 0x3B000000, 0x18000000)
        self.assertEqual(literal_target(word, 0x10274), 0x10288)

    def test_rust_pass_keeps_adjacent_only_guardrail(self):
        root = pathlib.Path(__file__).resolve().parents[2]
        source = (root / 'rust/crates/worker/src/preview.rs').read_text()
        self.assertIn('i==born.saturating_add(1)', source)
        self.assertIn('w>>31!=0&&(w&0x7f000000)==0x11000000', source)
        self.assertIn('sec.flags&0x8000_0000!=0||sec.flags&0x0000_0400!=0', source)
        self.assertIn('Interpretation guardrail', source)


if __name__ == '__main__':
    unittest.main()
