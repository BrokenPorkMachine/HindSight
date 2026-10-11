import ctypes as C
import ctypes.util
import os
from pathlib import Path
import random
import re
import struct
import subprocess
import tempfile
import unittest
from test_native import macho
ROOT=Path(__file__).resolve().parents[2]

def image(words=(0xd503201f,0xd65f03c0),extra=b'',extra_count=0,va=0x100000000):
    offset=184+len(extra);size=offset+4*len(words)
    segment=struct.pack('<II16sQQQQIIII',0x19,152,b'__TEXT',va,size,0,size,7,5,1,0)
    section=struct.pack('<16s16sQQIIIIIIII',b'__text',b'__TEXT',va+offset,len(words)*4,offset,2,0,0,0x80000400,0,0,0)
    return macho(segment+section+extra,1+extra_count)+struct.pack('<'+'I'*len(words),*words)

class DecoderTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory();out=Path(cls.temp.name)/'decoder.so'
        subprocess.run([os.environ.get('CC','cc'),'-std=c17','-Wall','-Wextra','-Werror','-shared','-fPIC',str(ROOT/'native/macho/decode.c'),str(ROOT/'native/macho/inspect.c'),'-o',str(out)],check=True)
        cls.lib=C.CDLL(str(out))
        cls.lib.re_arm64_decode.argtypes=[C.c_uint32,C.c_uint64,C.c_void_p,C.c_size_t]
        cls.lib.re_arm64_decode.restype=C.c_int
        cls.lib.re_macho_code_range.argtypes=[C.c_void_p,C.c_size_t,C.c_uint64,C.POINTER(C.c_uint64),C.POINTER(C.c_uint64),C.POINTER(C.c_uint64),C.c_void_p,C.c_size_t]
        cls.lib.re_macho_code_range.restype=C.c_int
    @classmethod
    def tearDownClass(cls):cls.temp.cleanup()
    def decode(self,word,pc=0x1000,capacity=256):
        out=C.create_string_buffer(capacity);status=self.lib.re_arm64_decode(word,pc,out,capacity)
        return status,out.value.decode()
    def mapping(self,data):
        offset=C.c_uint64();va=C.c_uint64();size=C.c_uint64();reason=C.create_string_buffer(512)
        status=self.lib.re_macho_code_range(data,len(data),len(data),C.byref(offset),C.byref(va),C.byref(size),reason,512)
        return status,offset.value,va.value,size.value,reason.value.decode()
    def test_control_flow(self):
        for word,expected in [(0xd503201f,'nop'),(0xd4000021,'svc #0x1'),(0xd4200040,'brk #0x2'),(0xd4400060,'hlt #0x3'),(0xd65f03c0,'ret x30'),(0xd61f0200,'br x16'),(0xd63f0220,'blr x17'),(0x14000002,'b 0x0000000000001008'),(0x97ffffff,'bl 0x0000000000000ffc'),(0x54000040,'b.eq 0x0000000000001008'),(0xb4ffffe0,'cbz x0, 0x0000000000000ffc'),(0x36080040,'tbz w0, #1, 0x0000000000001008')]:
            with self.subTest(word=hex(word)):self.assertEqual(self.decode(word),(1,expected))
    def test_data_immediates(self):
        for word,expected in [(0x10000020,'adr x0, 0x0000000000001004'),(0x90000020,'adrp x0, 0x0000000000005000'),(0xd2800020,'movz x0, #0x1, lsl #0'),(0x12800000,'movn w0, #0x0, lsl #0'),(0xf2a00040,'movk x0, #0x2, lsl #16'),(0x910043ff,'add sp, sp, #0x10, lsl #0'),(0xf100043f,'subs xzr, x1, #0x1, lsl #0')]:
            with self.subTest(word=hex(word)):self.assertEqual(self.decode(word),(1,expected))
    def test_unknown_is_not_guessed(self):
        for word in [0xffffffff,0,0x4e284800,0xd503233f,0x52c00000,0x32800000]:self.assertEqual(self.decode(word)[0],0)
    def test_address_overflow(self):
        self.assertEqual(self.decode(0x17ffffff,pc=0)[0],0)
        self.assertEqual(self.decode(0x14000001,pc=2**64-4)[0],0)
    def test_output_and_alignment(self):
        self.assertEqual(self.decode(0xd503201f,capacity=2)[0],-1)
        self.assertEqual(self.decode(0xd503201f,pc=1)[0],-1)
    def test_mapping(self):self.assertEqual(self.mapping(image())[:4],(0,184,0x1000000b8,8))
    def test_non_arm64(self):
        data=bytearray(image());struct.pack_into('<I',data,4,0x1000007)
        self.assertNotEqual(self.mapping(bytes(data))[0],0)
    def test_relocatable_not_mapped(self):
        data=bytearray(image());struct.pack_into('<I',data,12,1)
        self.assertNotEqual(self.mapping(bytes(data))[0],0)
    def test_modern_metadata_does_not_veto_mapping(self):
        # LC_FUNCTION_STARTS (0x26) and LC_DATA_IN_CODE (0x29) are normal modern
        # Mach-O metadata. A beta.6 regression confused 0x26 with data-in-code
        # and rejected nearly every production binary before analysis.
        for cmd in [struct.pack('<4I',0x26,16,0,8),struct.pack('<4I',0x29,16,0,8)]:
            result=self.mapping(image(extra=cmd,extra_count=1));self.assertEqual(result[0],0,result[-1])
    def test_true_encryption_is_metadata_only(self):
        result=self.mapping(image(extra=struct.pack('<6I',0x2c,24,0,0,1,0),extra_count=1))
        self.assertEqual(result[0],2);self.assertIn('encrypted',result[-1].lower())
    def test_mapping_mismatch(self):
        data=bytearray(image());struct.pack_into('<Q',data,32+72+32,0x1000000b4)
        result=self.mapping(bytes(data));self.assertNotEqual(result[0],0);self.assertIn('inconsistent',result[-1])
    def test_no_executable_section(self):
        data=bytearray(image());struct.pack_into('<I',data,32+60,1)
        self.assertNotEqual(self.mapping(bytes(data))[0],0)
    def test_mapping_truncation(self):
        data=image()
        for n in range(len(data)):self.assertNotEqual(self.mapping(data[:n])[0],0)
    def test_overlap_rejected(self):
        other=struct.pack('<II16sQQQQIIII',0x19,72,b'alias',0x100000000,1,0,1,7,1,0,0)
        # Overlap the selected code's file and virtual ranges rather than only its segment header.
        other=bytearray(other);struct.pack_into('<Q',other,24,0x100000100);struct.pack_into('<Q',other,40,256)
        result=self.mapping(image(extra=bytes(other),extra_count=1));self.assertNotEqual(result[0],0);self.assertIn('Overlapping',result[-1])
    def test_llvm_differential(self):
        path=os.environ.get('REWORKBENCH_LLVM_LIBRARY') or ctypes.util.find_library('LLVM-20')
        if not path:self.skipTest('Optional LLVM oracle unavailable; golden tests still run')
        llvm=C.CDLL(path)
        for name in ('LLVMInitializeAArch64TargetInfo','LLVMInitializeAArch64Target','LLVMInitializeAArch64TargetMC','LLVMInitializeAArch64Disassembler'):getattr(llvm,name)()
        llvm.LLVMCreateDisasm.argtypes=[C.c_char_p,C.c_void_p,C.c_int,C.c_void_p,C.c_void_p];llvm.LLVMCreateDisasm.restype=C.c_void_p
        llvm.LLVMDisasmInstruction.argtypes=[C.c_void_p,C.c_void_p,C.c_uint64,C.c_uint64,C.c_void_p,C.c_size_t];llvm.LLVMDisasmInstruction.restype=C.c_size_t
        llvm.LLVMDisasmDispose.argtypes=[C.c_void_p]
        context=llvm.LLVMCreateDisasm(b'aarch64-apple-darwin',None,0,None,None);self.assertTrue(context)
        rng=random.Random(4026);pc=0x100000000
        try:
            for _ in range(2000):
                word=rng.choice([0x14000000,0x94000000])|rng.randrange(1<<26)
                out=C.create_string_buffer(256)
                self.assertEqual(llvm.LLVMDisasmInstruction(context,word.to_bytes(4,'little'),4,pc,out,256),4)
                op,immediate=out.value.decode().split();expected=pc+int(immediate.lstrip('#'),0)
                self.assertEqual(self.decode(word,pc),(1,f'{op} 0x{expected:016x}'))
            for base,bits in [(0x54000000,19),(0xb4000000,19),(0x36080000,14)]:
                for _ in range(128):
                    word=base|(rng.randrange(1<<bits)<<5);out=C.create_string_buffer(256)
                    self.assertEqual(llvm.LLVMDisasmInstruction(context,word.to_bytes(4,'little'),4,pc,out,256),4)
                    raw=out.value.decode().strip();prefix,immediate=raw.rsplit('#',1)
                    prefix=' '.join(prefix.split());expected=pc+int(immediate,0)
                    self.assertEqual(self.decode(word,pc),(1,f'{prefix} 0x{expected:016x}'))
            decoded=0
            for _ in range(20000):
                word=rng.getrandbits(32)
                if self.decode(word,pc)[0]!=1:continue
                decoded+=1;out=C.create_string_buffer(256)
                self.assertEqual(llvm.LLVMDisasmInstruction(context,word.to_bytes(4,'little'),4,pc,out,256),4,f'overdecoded {word:08x}')
            self.assertGreater(decoded,100)
        finally:llvm.LLVMDisasmDispose(context)
