import ctypes as C
import hashlib
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
ROOT=Path(__file__).resolve().parents[2]
class PagingTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory();out=Path(cls.temp.name)/'paging.so'
        cmd=[os.environ.get('CC','cc'),'-std=c17','-Wall','-Wextra','-Werror','-shared','-fPIC',str(ROOT/'native/macho/capture.c'),str(ROOT/'native/macho/decode.c'),'-o',str(out)]
        cmd+=['-Wno-deprecated-declarations'] if sys.platform=='darwin' else ['-lcrypto']
        subprocess.run(cmd,check=True);cls.lib=C.CDLL(str(out))
        cls.lib.re_page_range.argtypes=[C.c_uint64]*4+[C.POINTER(C.c_uint64),C.POINTER(C.c_uint64),C.POINTER(C.c_uint32)]
        cls.lib.re_page_range.restype=C.c_int
        cls.lib.re_open_source.argtypes=[C.c_char_p];cls.lib.re_open_source.restype=C.c_int
        cls.lib.re_verify_fd.argtypes=[C.c_int,C.c_char_p,C.c_uint64,C.c_void_p,C.c_size_t];cls.lib.re_verify_fd.restype=C.c_int
    @classmethod
    def tearDownClass(cls):cls.temp.cleanup()
    def plan(self,words,start,offset=0x100,address=0x100001000):
        position=C.c_uint64();pc=C.c_uint64();count=C.c_uint32()
        status=self.lib.re_page_range(offset,address,words*4,start,C.byref(position),C.byref(pc),C.byref(count))
        return status,position.value,pc.value,count.value
    def test_first_middle_last(self):
        self.assertEqual(self.plan(600,0),(0,0x100,0x100001000,256))
        self.assertEqual(self.plan(600,256),(0,0x500,0x100001400,256))
        self.assertEqual(self.plan(600,512),(0,0x900,0x100001800,88))
        self.assertEqual(self.plan(600,599)[3],1)
    def test_exact_boundary(self):
        self.assertEqual(self.plan(512,256)[3],256)
        self.assertEqual(self.plan(512,512),(-1,0,0,0))
    def test_arbitrary_jump(self):self.assertEqual(self.plan(600,257),(0,0x504,0x100001404,256))
    def test_invalid_start(self):
        for size,start in [(0,0),(600,600),(600,2**64-1)]:self.assertEqual(self.plan(size,start),(-1,0,0,0))
    def test_extent_overflow(self):
        self.assertEqual(self.plan(2,0,address=2**64-4)[0],-1)
        self.assertEqual(self.plan(2,0,offset=2**64-4)[0],-1)
    def test_alignment(self):
        self.assertEqual(self.plan(1,0,address=3)[0],-1)
        self.assertEqual(self.plan(1,0,offset=3)[0],-1)
    def verify(self,fd,digest,size):
        error=C.create_string_buffer(512)
        status=self.lib.re_verify_fd(fd,digest.encode(),size,error,512)
        return status,error.value.decode()
    def test_verify_vectors_and_cursor(self):
        for data in (b'',b'abc',bytes(range(256))*8193):
            with self.subTest(size=len(data)),tempfile.TemporaryFile() as f:
                f.write(data);f.flush();f.seek(min(2,len(data)));cursor=f.tell()
                self.assertEqual(self.verify(f.fileno(),hashlib.sha256(data).hexdigest(),len(data))[0],0)
                self.assertEqual(f.tell(),cursor)
    def test_same_size_tampering(self):
        with tempfile.TemporaryFile() as f:
            original=b'good';f.write(b'evil');f.flush()
            result=self.verify(f.fileno(),hashlib.sha256(original).hexdigest(),4)
            self.assertEqual(result[0],-1);self.assertIn('SHA-256 mismatch',result[1])
    def test_wrong_size(self):
        with tempfile.TemporaryFile() as f:
            f.write(b'abc');f.flush()
            self.assertEqual(self.verify(f.fileno(),hashlib.sha256(b'abc').hexdigest(),4)[0],-1)
    def test_bad_digest_and_descriptor(self):
        with tempfile.TemporaryFile() as f:
            self.assertEqual(self.verify(f.fileno(),'bad',0)[0],-1)
        self.assertEqual(self.verify(-1,'0'*64,0)[0],-1)
    def test_open_refuses_symlink_and_fifo(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);file=root/'file';link=root/'link';fifo=root/'fifo';file.write_bytes(b'a');link.symlink_to(file);os.mkfifo(fifo)
            for path in (link,fifo,root):self.assertEqual(self.lib.re_open_source(os.fsencode(path)),-1)
    def test_open_regular_owned_descriptor(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'file';path.write_bytes(b'abc');fd=self.lib.re_open_source(os.fsencode(path));self.assertGreaterEqual(fd,0)
            try:self.assertEqual(os.read(fd,3),b'abc')
            finally:os.close(fd)
