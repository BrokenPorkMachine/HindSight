import ctypes as C
import os
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest
from test_native import macho
ROOT=Path(__file__).resolve().parents[2]
class CacheTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory();out=Path(cls.temp.name)/'cache.so'
        subprocess.run([os.environ.get('CC','cc'),'-std=c17','-Wall','-Wextra','-Werror','-shared','-fPIC',str(ROOT/'native/macho/cache.c'),str(ROOT/'native/macho/inspect.c'),'-o',str(out)],check=True)
        cls.lib=C.CDLL(str(out));l=cls.lib
        l.re_cache_new.argtypes=[C.c_size_t,C.c_size_t];l.re_cache_new.restype=C.c_void_p
        for name in ('re_cache_free','re_cache_clear'):getattr(l,name).argtypes=[C.c_void_p];getattr(l,name).restype=None
        l.re_cache_put.argtypes=[C.c_void_p,C.c_void_p,C.c_size_t,C.c_void_p,C.c_size_t];l.re_cache_put.restype=C.c_int
        l.re_cache_get.argtypes=[C.c_void_p,C.c_void_p,C.c_size_t,C.POINTER(C.c_size_t)];l.re_cache_get.restype=C.c_void_p
        for name in ('re_cache_bytes','re_cache_count'):getattr(l,name).argtypes=[C.c_void_p];getattr(l,name).restype=C.c_size_t
        l.re_file_stamp.argtypes=[C.c_char_p,C.c_void_p,C.c_size_t];l.re_file_stamp.restype=C.c_int
        l.re_macho_window_size.argtypes=[C.c_void_p,C.c_size_t,C.c_uint64];l.re_macho_window_size.restype=C.c_size_t
    @classmethod
    def tearDownClass(cls):cls.temp.cleanup()
    def setUp(self):self.cache=self.lib.re_cache_new(1024,3);self.assertTrue(self.cache)
    def tearDown(self):self.lib.re_cache_free(self.cache)
    def put(self,key,value):return self.lib.re_cache_put(self.cache,key,len(key),value,len(value))
    def get(self,key):
        n=C.c_size_t(999);pointer=self.lib.re_cache_get(self.cache,key,len(key),C.byref(n))
        if not pointer:self.assertEqual(n.value,0);return None
        return C.string_at(pointer,n.value)
    def test_roundtrip_and_missing(self):
        self.assertIsNone(self.get(b'missing'));self.assertEqual(self.put(b'key',b'value'),1);self.assertEqual(self.get(b'key'),b'value')
    def test_lru_touch(self):
        for key in (b'a',b'b',b'c'):self.put(key,key)
        self.get(b'a');self.put(b'd',b'd')
        self.assertIsNone(self.get(b'b'));self.assertEqual(self.get(b'a'),b'a');self.assertEqual(self.lib.re_cache_count(self.cache),3)
    def test_byte_budget_eviction(self):
        self.put(b'a',b'a'*500);self.put(b'b',b'b'*500)
        self.assertIsNone(self.get(b'a'));self.assertLessEqual(self.lib.re_cache_bytes(self.cache),1024)
    def test_replace(self):
        self.put(b'a',b'old');self.put(b'a',b'new')
        self.assertEqual(self.lib.re_cache_count(self.cache),1);self.assertEqual(self.get(b'a'),b'new')
    def test_clear(self):
        self.put(b'a',b'b');self.lib.re_cache_clear(self.cache)
        self.assertIsNone(self.get(b'a'));self.assertEqual(self.lib.re_cache_bytes(self.cache),0);self.assertEqual(self.lib.re_cache_count(self.cache),0)
    def test_oversize_preserves_existing(self):
        self.put(b'a',b'keep');self.assertEqual(self.put(b'a',b'x'*1024),0);self.assertEqual(self.get(b'a'),b'keep')
    def test_binary_key_identity(self):
        self.put(b'a\0b',b'first');self.put(b'a\0c',b'second')
        self.assertEqual(self.get(b'a\0b'),b'first');self.assertEqual(self.get(b'a\0c'),b'second')
    def test_configuration_caps(self):
        for size,count in [(0,1),(1024,0),(1024*1024+1,16),(1024,17)]:self.assertFalse(self.lib.re_cache_new(size,count))
    def stamp(self,path):
        out=C.create_string_buffer(512);code=self.lib.re_file_stamp(os.fsencode(path),out,512);return code,out.value
    def test_stamp_changes_and_rejects_symlink(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'object';p.write_bytes(b'a');first=self.stamp(p);self.assertEqual(first[0],0);self.assertEqual(first,self.stamp(p))
            p.write_bytes(b'changed');self.assertNotEqual(first,self.stamp(p))
            link=Path(d)/'link';link.symlink_to(p);self.assertEqual(self.stamp(link)[0],-1);self.assertEqual(self.stamp(Path(d))[0],-1)
    def test_stamp_replacement(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'object';p.write_bytes(b'a');first=self.stamp(p)
            replacement=Path(d)/'other';replacement.write_bytes(b'a');os.replace(replacement,p)
            self.assertNotEqual(first,self.stamp(p))
    def window(self,header,size):return self.lib.re_macho_window_size(header,len(header),size)
    def test_window_small_unknown(self):
        self.assertEqual(self.window(b'',0),0);self.assertEqual(self.window(b'abc',3),3);self.assertEqual(self.window(b'x'*32,8*1024**3),32)
    def test_window_exact_commands(self):
        for endian in ('<','>'):
            h=macho(struct.pack(endian+'II',0,8),1,endian)[:32]
            self.assertEqual(self.window(h,8*1024**3),40)
    def test_window_truncation_and_budget(self):
        h=bytearray(macho());struct.pack_into('<I',h,20,1000)
        self.assertEqual(self.window(bytes(h),45),45)
        struct.pack_into('<I',h,20,16*1024*1024+1);self.assertEqual(self.window(bytes(h),8*1024**3),32)
        struct.pack_into('<I',h,20,16*1024*1024);self.assertEqual(self.window(bytes(h),8*1024**3),16*1024*1024+32)
        struct.pack_into('<I',h,16,8193);self.assertEqual(self.window(bytes(h),8*1024**3),32)
