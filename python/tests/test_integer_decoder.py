import ctypes as C
import ctypes.util
import os
import random
import re
import unittest
import test_decoder

class Oracle:
    def __init__(self):
        path=os.environ.get('REWORKBENCH_LLVM_LIBRARY') or ctypes.util.find_library('LLVM-20')
        if not path:raise unittest.SkipTest('Optional LLVM oracle unavailable')
        self.lib=C.CDLL(path)
        for name in ('LLVMInitializeAArch64TargetInfo','LLVMInitializeAArch64Target','LLVMInitializeAArch64TargetMC','LLVMInitializeAArch64Disassembler'):getattr(self.lib,name)()
        self.lib.LLVMCreateDisasm.argtypes=[C.c_char_p,C.c_void_p,C.c_int,C.c_void_p,C.c_void_p];self.lib.LLVMCreateDisasm.restype=C.c_void_p
        self.lib.LLVMDisasmInstruction.argtypes=[C.c_void_p,C.c_void_p,C.c_uint64,C.c_uint64,C.c_void_p,C.c_size_t];self.lib.LLVMDisasmInstruction.restype=C.c_size_t
        self.lib.LLVMDisasmDispose.argtypes=[C.c_void_p]
        self.context=self.lib.LLVMCreateDisasm(b'aarch64-apple-darwin',None,0,None,None)
        if not self.context:raise RuntimeError('LLVM could not create the AArch64 disassembler')
    def close(self):self.lib.LLVMDisasmDispose(self.context)
    def decode(self,word,pc=0x100000000):
        out=C.create_string_buffer(256)
        count=self.lib.LLVMDisasmInstruction(self.context,word.to_bytes(4,'little'),4,pc,out,256)
        if count!=4:raise AssertionError(f'Oracle refused {word:08x}')
        return out.value.decode().strip()

def normalize(text):
    text=' '.join(text.split()).replace(', lsl #0','').replace(', #0]',']')
    return re.sub(r'#(-?0x[0-9a-f]+|-?\d+)',lambda m:'#'+str(int(m[1],0)),text)

class IntegerDecoderTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):test_decoder.DecoderTests.setUpClass.__func__(cls)
    @classmethod
    def tearDownClass(cls):test_decoder.DecoderTests.tearDownClass.__func__(cls)
    decode=test_decoder.DecoderTests.decode
    def golden(self,rows):
        for word,expected in rows:
            with self.subTest(word=f'{word:08x}'):self.assertEqual(self.decode(word),(1,expected))
    def test_stack_prologue_epilogue(self):
        self.golden([(0xa9bf7bfd,'stp x29, x30, [sp, #-16]!'),(0xa8c17bfd,'ldp x29, x30, [sp], #16'),(0xf9400fe0,'ldr x0, [sp, #24]'),(0xb9000be1,'str w1, [sp, #8]')])
    def test_sign_extending_loads(self):
        self.golden([(0x39800420,'ldrsb x0, [x1, #1]'),(0x39c00420,'ldrsb w0, [x1, #1]'),(0x79800420,'ldrsh x0, [x1, #2]'),(0x79c00420,'ldrsh w0, [x1, #2]'),(0xb9800420,'ldrsw x0, [x1, #4]')])
    def test_memory_address_modes(self):
        self.golden([(0x39400c20,'ldrb w0, [x1, #3]'),(0xf85f8020,'ldur x0, [x1, #-8]'),(0xf8408420,'ldr x0, [x1], #8'),(0xf81f8fe0,'str x0, [sp, #-8]!')])
    def test_scaled_offset_limits(self):
        self.golden([(0xf97ffc20,'ldr x0, [x1, #32760]'),(0xf8500020,'ldur x0, [x1, #-256]'),(0xf84ff020,'ldur x0, [x1, #255]')])
    def test_zero_register_not_stack_pointer(self):
        self.golden([(0xf90003ff,'str xzr, [sp, #0]'),(0x8b1f03e0,'add x0, xzr, xzr, lsl #0'),(0xaa0103e0,'orr x0, xzr, x1, lsl #0')])
    def test_register_arithmetic(self):
        self.golden([(0x8b020020,'add x0, x1, x2, lsl #0'),(0xcb820c20,'sub x0, x1, x2, asr #3'),(0x8a220020,'bic x0, x1, x2, lsl #0')])
    def test_literal_loads(self):
        self.golden([(0x18000040,'ldr w0, 0x0000000000001008'),(0x58000040,'ldr x0, 0x0000000000001008'),(0x98000040,'ldrsw x0, 0x0000000000001008'),(0x58ffffe0,'ldr x0, 0x0000000000000ffc')])
        self.assertEqual(self.decode(0x58ffffe0,pc=0)[0],0)
    def test_reserved_and_excluded_forms(self):
        for word in [0xf9800020,0xd8000000,0x3dc00020,0x0bc00020,0x0b028020,0x0a028020,0xf8400820,0x69400820]:
            with self.subTest(word=f'{word:08x}'):self.assertEqual(self.decode(word)[0],0)
    def test_writeback_and_load_pair_overlap(self):
        for word in [0xf8408400,0xf8000c00,0xa9bf0400,0xa9400000]:
            with self.subTest(word=f'{word:08x}'):self.assertEqual(self.decode(word)[0],0)
    def test_bundled_sample(self):
        data=(test_decoder.ROOT/'Tests/Fixtures/arm64-integer-sample.macho').read_bytes()
        status,offset,address,size,_=test_decoder.DecoderTests.mapping(self,data)
        self.assertEqual((status,offset,address,size),(0,184,0x1000000b8,64))
        decoded=[self.decode(int.from_bytes(data[offset+i:offset+i+4],'little'),address+i) for i in range(0,size,4)]
        self.assertEqual([value[0] for value in decoded],[1]*13+[0]*3)
        self.assertEqual(decoded[0][1],'stp x29, x30, [sp, #-16]!')
        self.assertEqual(decoded[11][1],'ldp x29, x30, [sp], #16')
    def compare(self,oracle,word):
        result,text=self.decode(word,pc=0x100000000)
        self.assertEqual(result,1,f'not decoded {word:08x}')
        self.assertEqual(normalize(text),normalize(oracle.decode(word)),f'{word:08x}')
    def test_llvm_memory_operands(self):
        oracle=Oracle();rng=random.Random(7001);count=0
        try:
            for size,opcs in [(0,range(4)),(1,range(4)),(2,range(3)),(3,range(2))]:
                for opc in opcs:
                    for mode in ('unsigned',0,1,3):
                        for _ in range(64):
                            rt=rng.randrange(28);rn=rng.choice([n for n in range(32) if n!=rt])
                            if mode=='unsigned':word=0x39000000|(size<<30)|(opc<<22)|(rng.randrange(4096)<<10)|(rn<<5)|rt
                            else:word=0x38000000|(size<<30)|(opc<<22)|(rng.randrange(512)<<12)|(mode<<10)|(rn<<5)|rt
                            self.compare(oracle,word);count+=1
            self.assertEqual(count,3328)
        finally:oracle.close()
    def test_llvm_pair_operands(self):
        oracle=Oracle();rng=random.Random(7002);count=0
        try:
            for opc in (0,2):
                for load in (0,1):
                    for mode in range(4):
                        for _ in range(64):
                            rt=rng.randrange(28);rt2=rng.choice([n for n in range(28) if n!=rt]);rn=rng.choice([n for n in range(32) if n not in (rt,rt2)])
                            word=0x28000000|(opc<<30)|(mode<<23)|(load<<22)|(rng.randrange(128)<<15)|(rt2<<10)|(rn<<5)|rt
                            self.compare(oracle,word);count+=1
            self.assertEqual(count,1024)
        finally:oracle.close()
    def test_llvm_shifted_arithmetic_operands(self):
        oracle=Oracle();rng=random.Random(7003);count=0
        try:
            for sf in (0,1):
                for sub in (0,1):
                    for flags in (0,1):
                        for shift in range(3):
                            for _ in range(16):
                                rd,rn,rm=[rng.randrange(28) for _ in range(3)];amount=rng.randrange(64 if sf else 32)
                                word=0x0b000000|(sf<<31)|(sub<<30)|(flags<<29)|(shift<<22)|(rm<<16)|(amount<<10)|(rn<<5)|rd
                                self.compare(oracle,word);count+=1
            self.assertEqual(count,384)
        finally:oracle.close()
    def test_llvm_logical_operands(self):
        oracle=Oracle();rng=random.Random(7004);count=0
        try:
            for sf in (0,1):
                for opc in range(4):
                    for invert in (0,1):
                        for shift in range(4):
                            for _ in range(16):
                                rd,rn,rm=[rng.randrange(28) for _ in range(3)];amount=rng.randrange(64 if sf else 32)
                                word=0x0a000000|(sf<<31)|(opc<<29)|(shift<<22)|(invert<<21)|(rm<<16)|(amount<<10)|(rn<<5)|rd
                                self.compare(oracle,word);count+=1
            self.assertEqual(count,1024)
        finally:oracle.close()
    def test_llvm_literal_targets(self):
        oracle=Oracle();rng=random.Random(7005);pc=0x100000000;count=0
        try:
            for opc in range(3):
                for _ in range(128):
                    word=0x18000000|(opc<<30)|(rng.randrange(1<<19)<<5)|rng.randrange(28)
                    raw=oracle.decode(word,pc);prefix,immediate=raw.rsplit('#',1)
                    expected=' '.join(prefix.split())+f' 0x{pc+int(immediate,0):016x}'
                    self.assertEqual(self.decode(word,pc),(1,expected));count+=1
            self.assertEqual(count,384)
        finally:oracle.close()
