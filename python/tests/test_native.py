"""Execute the actual native capture and range parser, without a Rust toolchain."""
import ctypes as C
import hashlib
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]

def macho(commands=b'', count=0, endian='<'):
    return struct.pack(endian+'8I', 0xfeedfacf,0x100000c,0,2,count,len(commands),0,0)+commands

def segment(va=0x1000, size=104, offset=0, file_size=104):
    return struct.pack('<II16sQQQQIIII',0x19,72,b'__TEXT',va,size,offset,file_size,7,5,0,0)

class NativeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory()
        output=Path(cls.temp.name)/'native.so'
        cmd=[os.environ.get('CC','cc'),'-std=c17','-Wall','-Wextra','-Werror','-shared','-fPIC',str(ROOT/'native/macho/capture.c'),str(ROOT/'native/macho/inspect.c'),'-o',str(output)]
        if sys.platform!='darwin': cmd+=['-lcrypto']
        else: cmd+=['-Wno-deprecated-declarations']
        subprocess.run(cmd,check=True)
        cls.lib=C.CDLL(str(output))
        cls.lib.re_capture.argtypes=[C.c_char_p,C.c_char_p,C.c_uint64,C.c_uint64,C.c_void_p,C.c_void_p,C.c_size_t]
        cls.lib.re_capture.restype=C.c_int
        cls.lib.re_macho_inspect.argtypes=[C.c_void_p,C.c_size_t,C.c_uint64,C.c_void_p,C.c_size_t]
        cls.lib.re_macho_inspect.restype=C.c_int
    @classmethod
    def tearDownClass(cls): cls.temp.cleanup()
    def inspect(self,data,size=None,capacity=65536):
        output=C.create_string_buffer(capacity)
        code=self.lib.re_macho_inspect(data,len(data),len(data) if size is None else size,output,capacity)
        return code,output.value.decode()
    def capture(self,source,destination,maximum=8*1024**3,reserve=0):
        digest=C.create_string_buffer(65); error=C.create_string_buffer(512)
        code=self.lib.re_capture(os.fsencode(source),os.fsencode(destination),maximum,reserve,digest,error,512)
        return code,digest.value.decode(),error.value.decode()
    def test_capture_vectors(self):
        for data in (b'',b'abc',bytes(range(256))*8193):
            with self.subTest(size=len(data)),tempfile.TemporaryDirectory() as folder:
                src=Path(folder)/'in';dst=Path(folder)/'out';src.write_bytes(data)
                code,digest,error=self.capture(src,dst)
                self.assertEqual(code,0,error);self.assertEqual(digest,hashlib.sha256(data).hexdigest())
                self.assertEqual(dst.read_bytes(),data);self.assertEqual(dst.stat().st_mode&0o777,0o444)
    def test_existing_destination_preserved(self):
        with tempfile.TemporaryDirectory() as f:
            src=Path(f)/'in';dst=Path(f)/'out';src.write_bytes(b'abc');dst.write_bytes(b'keep')
            self.assertEqual(self.capture(src,dst)[0],-1);self.assertEqual(dst.read_bytes(),b'keep')
    def test_symlink_source_rejected(self):
        with tempfile.TemporaryDirectory() as f:
            src=Path(f)/'in';link=Path(f)/'link';dst=Path(f)/'out';src.write_bytes(b'a');link.symlink_to(src)
            self.assertEqual(self.capture(link,dst)[0],-1);self.assertFalse(dst.exists())
    def test_nonregular_source_rejected(self):
        with tempfile.TemporaryDirectory() as f:
            self.assertEqual(self.capture(f,Path(f)/'out')[0],-1)
    def test_size_limit(self):
        with tempfile.TemporaryDirectory() as f:
            src=Path(f)/'in';dst=Path(f)/'out';src.write_bytes(b'ab')
            self.assertEqual(self.capture(src,dst,maximum=1)[0],-1);self.assertFalse(dst.exists())
    def test_headroom_failure_cleans_owned_file(self):
        with tempfile.TemporaryDirectory() as f:
            src=Path(f)/'in';dst=Path(f)/'out';src.write_bytes(b'')
            self.assertEqual(self.capture(src,dst,reserve=2**64-1)[0],-1);self.assertFalse(dst.exists())
    def test_thin_both_endians(self):
        for endian in ('<','>'): self.assertEqual(self.inspect(macho(endian=endian))[0],0)
    def test_segment(self):
        code,text=self.inspect(macho(segment(),1));self.assertEqual(code,0);self.assertIn('__TEXT',text)
    def test_segment_overflow(self): self.assertEqual(self.inspect(macho(segment(va=2**64-10),1))[0],-1)
    def test_segment_outside_file(self): self.assertEqual(self.inspect(macho(segment(offset=100),1))[0],-1)
    def test_truncation(self):
        data=macho(segment(),1)
        for n in range(4,len(data)): self.assertEqual(self.inspect(data[:n])[0],-1)
    def test_bad_command_size(self): self.assertEqual(self.inspect(macho(struct.pack('<II',0,0),1))[0],-1)
    def test_command_count_mismatch(self): self.assertEqual(self.inspect(macho(struct.pack('<II',0,8),0))[0],-1)
    def test_symbol_range(self): self.assertEqual(self.inspect(macho(struct.pack('<6I',2,24,56,1,56,0),1))[0],-1)
    def test_encryption_report(self):
        code,text=self.inspect(macho(struct.pack('<6I',0x2c,24,0,0,1,0),1));self.assertEqual(code,0);self.assertIn('Encryption ID: 1',text)
    def test_universal_unsupported(self): self.assertEqual(self.inspect(bytes.fromhex('cafebabe'))[0],1)
    def test_output_budget(self): self.assertEqual(self.inspect(macho(),capacity=8)[0],-1)
    def test_section(self):
        cmd=struct.pack('<II16sQQQQIIII',0x19,152,b'__TEXT',0x1000,188,0,188,7,5,1,0)
        section=struct.pack('<16s16sQQIIIIIIII',b'__text',b'__TEXT',0x1000+184,4,184,2,0,0,0,0,0,0)
        data=macho(cmd+section,1)+b'code'
        self.assertEqual(self.inspect(data)[0],0)
        malformed=bytearray(data);struct.pack_into('<Q',malformed,32+72+40,5)
        self.assertEqual(self.inspect(bytes(malformed))[0],-1)
