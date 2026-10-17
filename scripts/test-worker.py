#!/usr/bin/env python3
import io, pathlib, subprocess, sys
sys.path.insert(0,str(pathlib.Path(__file__).resolve().parents[1]/"python"))
from reworkbench_sdk import read_frame, write_frame
request=io.BytesIO()
write_frame(request,b'wire interoperability')
write_frame(request,b'')
reply=subprocess.run([sys.argv[1],"--echo-frames"],input=request.getvalue(),capture_output=True,timeout=5,check=True)
stream=io.BytesIO(reply.stdout)
assert read_frame(stream)==b'wire interoperability'
assert read_frame(stream)==b''
assert read_frame(stream) is None
bad=subprocess.run([sys.argv[1],"--echo-frames"],input=b'\xff\xff\xff\xff',capture_output=True,timeout=5)
assert bad.returncode != 0
print('Rust/Python frame interoperability passed')

import json, tempfile, uuid
with tempfile.TemporaryDirectory() as directory:
    path=pathlib.Path(directory)/'fixture.bin'
    path.write_bytes(bytes.fromhex('cffaedfe0c000001')+bytes(24))
    request=str(uuid.uuid4())
    result=subprocess.run([sys.argv[1],'--inspect-header',str(path),request],capture_output=True,timeout=5,check=True)
    stream=io.BytesIO(result.stdout);response=json.loads(read_frame(stream))
    assert read_frame(stream) is None
    assert response['request_id']==request and response['protocol']==1
    assert response['architecture']=='ARM64' and response['size']=='32'
    assert response['captured'] is False
    missing=subprocess.run([sys.argv[1],'--inspect-header',str(path.parent/'missing'),request],capture_output=True,timeout=5,check=True)
    assert json.loads(read_frame(io.BytesIO(missing.stdout)))['kind']=='error'
print('Header worker inspection and error fixtures passed')

import hashlib
with tempfile.TemporaryDirectory() as directory:
    source=pathlib.Path(directory)/'source.bin'; destination=pathlib.Path(directory)/'captured.bin'
    source.write_bytes(bytes.fromhex('cffaedfe0c000001')+bytes(24))
    request=str(uuid.uuid4())
    process=subprocess.run([sys.argv[1],'--capture-inspect',str(source),str(destination),request],capture_output=True,timeout=120,check=True)
    stream=io.BytesIO(process.stdout); response=json.loads(read_frame(stream))
    assert read_frame(stream) is None
    assert response['kind']=='header_result',response
    assert response['captured'] is True and response['request_id']==request
    assert response['source_sha256']==hashlib.sha256(source.read_bytes()).hexdigest()
    assert response['structure_status']=='checked'
    assert destination.read_bytes()==source.read_bytes()
    source.write_bytes(b'changed')
    assert destination.read_bytes()!=source.read_bytes()
print('Source capture, identity and layout worker fixtures passed')

# End-to-end preview fixture (requires a compiled worker and >2 GiB free space).
sys.path.insert(0,str(pathlib.Path(__file__).resolve().parents[1]/'python/tests'))
from test_decoder import image
with tempfile.TemporaryDirectory() as directory:
    source=pathlib.Path(directory)/'arm64.bin';destination=pathlib.Path(directory)/'capture.bin'
    source.write_bytes(image(words=(0xd503201f,0x14000001,0xd65f03c0,0xffffffff)))
    request=str(uuid.uuid4())
    process=subprocess.run([sys.argv[1],'--capture-inspect',str(source),str(destination),request],capture_output=True,timeout=120,check=True)
    stream=io.BytesIO(process.stdout);response=json.loads(read_frame(stream));assert read_frame(stream) is None
    assert response['disassembly_status']=='preview',response
    assert '00000001000000b8  00000000000000b8' in response['disassembly']
    assert 'b 0x00000001000000c0' in response['disassembly']
    assert 'ret x30' in response['disassembly'] and '.inst 0xffffffff' in response['disassembly']
    assert '4 of 4 words shown; 3 decoded, 1 unsupported.' in response['disassembly']
print('ARM64 preview, file/VA mapping and unsupported-word fixture passed')

# On-demand pages use existing captures; no staging copy or database write is needed.
with tempfile.TemporaryDirectory() as directory:
    source=pathlib.Path(directory)/'large-arm64.bin'
    data=image(words=(0xd503201f,)*599+(0xd65f03c0,));source.write_bytes(data)
    digest=hashlib.sha256(data).hexdigest()
    def page(start,expected_digest=digest):
        identity=str(uuid.uuid4())
        process=subprocess.run([sys.argv[1],'--page',str(source),expected_digest,str(len(data)),str(start),identity],capture_output=True,timeout=120,check=True)
        stream=io.BytesIO(process.stdout);response=json.loads(read_frame(stream));assert read_frame(stream) is None
        if response['kind']=='page_result': assert response['request_id']==identity and response['source_sha256']==expected_digest
        return response
    for start,count in [(0,256),(256,256),(512,88),(599,1)]:
        response=page(start);assert response['word_count']==count and response['start_word']==str(start),response
        assert response['total_words']=='600'
    assert 'ret x30' in page(599)['text']
    assert page(600)['kind']=='error'
    assert page(2**64-1)['kind']=='error'
    assert page(0,'0'*64)['kind']=='error'
    source.write_bytes(data[:-4]+bytes(4))
    assert page(0)['kind']=='error'
print('Paged worker boundaries and captured-object tamper checks passed')

with tempfile.TemporaryDirectory() as directory:
    source=pathlib.Path(__file__).resolve().parents[1]/'Tests/Fixtures/arm64-integer-sample.macho'
    destination=pathlib.Path(directory)/'sample.capture'
    process=subprocess.run([sys.argv[1],'--capture-inspect',str(source),str(destination),str(uuid.uuid4())],capture_output=True,timeout=120,check=True)
    stream=io.BytesIO(process.stdout);response=json.loads(read_frame(stream));assert read_frame(stream) is None
    assert response['disassembly_status']=='preview',response
    assert 'stp x29, x30, [sp, #-16]!' in response['disassembly']
    assert 'ldp x29, x30, [sp], #16' in response['disassembly']
    assert '16 of 16 words shown; 13 decoded, 3 unsupported.' in response['disassembly']
    assert 'A64 subset v2' in response['disassembly']
print('Integer decoder fixture capture and preview passed')

source = pathlib.Path(__file__).resolve().parents[1] / 'Tests/Fixtures/arm64-integer-sample.macho'
data = source.read_bytes()
process = subprocess.run(
    [sys.argv[1], '--analyze', str(source), hashlib.sha256(data).hexdigest(),
     str(len(data)), str(uuid.uuid4())],
    capture_output=True, timeout=120, check=True,
)
report = json.loads(read_frame(io.BytesIO(process.stdout)))
assert report['kind'] == 'analysis_result'
semantic = report['semantic_preview']
assert 'function 0x00000001000000b8' in semantic
assert '0x00000001000000cc  x0 = x0 + x1;' in semantic
assert 'word=8b010000' in semantic
assert 'opaque instruction: semantics unavailable' in semantic
assert 'Reconstructed pseudocode' in semantic
assert 'signature unresolved' in semantic
assert 'Signature evidence:' in report['call_values']
assert b'"source":"Semantic Preview"' in process.stderr
assert b'"address":"0x00000001000000cc"' in process.stderr
print('ARM64 instruction-derived semantic preview and provenance passed')

with tempfile.TemporaryDirectory() as directory:
    source = pathlib.Path(directory) / 'straight-line.macho'
    data = image(words=(0x52800020, 0x11000400, 0xd65f03c0))
    source.write_bytes(data)
    process = subprocess.run(
        [sys.argv[1], '--analyze', str(source), hashlib.sha256(data).hexdigest(),
         str(len(data)), str(uuid.uuid4())],
        capture_output=True, timeout=120, check=True,
    )
    report = json.loads(read_frame(io.BytesIO(process.stdout)))
    semantic = report['semantic_preview']
    assert 'AArch64 register semantics v45' in semantic
    assert 'constant=0x2 proven_by=[0x1000000b8,0x1000000bc]' in semantic
    assert 'ABI result register x0 /* constant=0x2 proven_by=[0x1000000b8,0x1000000bc] */' in semantic
print('ARM64 straight-line constant provenance passed through worker analysis')

with tempfile.TemporaryDirectory() as directory:
    source = pathlib.Path(directory) / 'move-wide.macho'
    data = image(words=(0xd2824680, 0xf2aacf00, 0xd65f03c0))
    source.write_bytes(data)
    process = subprocess.run(
        [sys.argv[1], '--analyze', str(source), hashlib.sha256(data).hexdigest(),
         str(len(data)), str(uuid.uuid4())],
        capture_output=True, timeout=120, check=True,
    )
    report = json.loads(read_frame(io.BytesIO(process.stdout)))
    semantic = report['semantic_preview']
    assert 'AArch64 register semantics v45' in semantic
    assert 'x0 = (x0 & 0xffffffff0000ffff) | 0x56780000;' in semantic
    assert 'constant=0x56781234 proven_by=[0x1000000b8,0x1000000bc]' in semantic
    assert 'word=f2aacf00' in semantic
print('ARM64 move-wide constant materialization passed through worker analysis')

with tempfile.TemporaryDirectory() as directory:
    source = pathlib.Path(directory) / 'logical.macho'
    data = image(words=(0x12000920, 0x52801fe0, 0x528001e1, 0x4a010000, 0xd65f03c0))
    source.write_bytes(data)
    process = subprocess.run(
        [sys.argv[1], '--analyze', str(source), hashlib.sha256(data).hexdigest(),
         str(len(data)), str(uuid.uuid4())],
        capture_output=True, timeout=120, check=True,
    )
    report = json.loads(read_frame(io.BytesIO(process.stdout)))
    semantic = report['semantic_preview']
    assert 'AArch64 register semantics v45' in semantic
    assert 'w0 = w9 & 0x7;' in semantic
    assert 'word=12000920' in semantic
    assert 'w0 = w0 ^ w1; /* constant=0xf0 proven_by=[0x1000000bc,0x1000000c0,0x1000000c4] */' in semantic
    assert 'ABI result register x0 /* constant=0xf0 proven_by=[0x1000000bc,0x1000000c0,0x1000000c4] */' in semantic
print('ARM64 logical operations passed through worker analysis')

with tempfile.TemporaryDirectory() as directory:
    source = pathlib.Path(directory) / 'pc-relative.macho'
    data = image(words=(0xb0000000, 0x91004000, 0xd65f03c0))
    source.write_bytes(data)
    process = subprocess.run(
        [sys.argv[1], '--analyze', str(source), hashlib.sha256(data).hexdigest(),
         str(len(data)), str(uuid.uuid4())],
        capture_output=True, timeout=120, check=True,
    )
    report = json.loads(read_frame(io.BytesIO(process.stdout)))
    semantic = report['semantic_preview']
    assert 'AArch64 register semantics v45' in semantic
    assert 'x0 = 0x100001000; /* pc-relative page */' in semantic
    assert 'word=b0000000' in semantic
    assert 'x0 = x0 + 0x10; /* constant=0x100001010 proven_by=[0x1000000b8,0x1000000bc] */' in semantic
    assert 'ABI result register x0 /* constant=0x100001010 proven_by=[0x1000000b8,0x1000000bc] */' in semantic
print('ARM64 PC-relative address materialization passed through worker analysis')

with tempfile.TemporaryDirectory() as directory:
    source = pathlib.Path(directory) / 'control-flow.macho'
    data = image(words=(0x54000140, 0x34000100, 0x361800c2, 0xd63f0120, 0xd61f0100, 0xd65f03c0))
    source.write_bytes(data)
    process = subprocess.run(
        [sys.argv[1], '--analyze', str(source), hashlib.sha256(data).hexdigest(),
         str(len(data)), str(uuid.uuid4())],
        capture_output=True, timeout=120, check=True,
    )
    report = json.loads(read_frame(io.BytesIO(process.stdout)))
    semantic = report['semantic_preview']
    assert 'AArch64 register semantics v45' in semantic
    assert 'if (eq) goto 0x1000000e0;' in semantic
    assert 'if (w0 == 0) goto 0x1000000dc;' in semantic
    assert 'if ((w2 & 0x8) == 0) goto 0x1000000d8;' in semantic
    assert 'call(x9); /* indirect; signature, arguments, result and side effects unresolved */' in semantic
    assert 'goto x8; /* indirect target unresolved */' in semantic
    assert 'word=54000140' in semantic
print('ARM64 conditional and indirect control flow passed through worker analysis')

with tempfile.TemporaryDirectory() as directory:
    source = pathlib.Path(directory) / 'indirect-target.macho'
    data = image(words=(0xd2800608, 0xd61f0100, 0xd2800809, 0xd63f0120))
    source.write_bytes(data)
    process = subprocess.run(
        [sys.argv[1], '--analyze', str(source), hashlib.sha256(data).hexdigest(),
         str(len(data)), str(uuid.uuid4())],
        capture_output=True, timeout=120, check=True,
    )
    report = json.loads(read_frame(io.BytesIO(process.stdout)))
    semantic = report['semantic_preview']
    assert 'AArch64 register semantics v45' in semantic
    assert 'goto 0x30; /* indirect via x8; proven_by=[0x1000000b8] */' in semantic
    assert 'call(0x40); /* indirect via x9; proven_by=[0x1000000c0]; signature, arguments, result and side effects unresolved */' in semantic
    assert 'word=d61f0100' in semantic
    assert 'word=d63f0120' in semantic
print('ARM64 indirect-target resolution passed through worker analysis')

with tempfile.TemporaryDirectory() as directory:
    source = pathlib.Path(directory) / 'shifts.macho'
    data = image(words=(0x531d7062, 0x53037d28, 0x13037dee, 0x12800001, 0x13047c22))
    source.write_bytes(data)
    process = subprocess.run(
        [sys.argv[1], '--analyze', str(source), hashlib.sha256(data).hexdigest(),
         str(len(data)), str(uuid.uuid4())],
        capture_output=True, timeout=120, check=True,
    )
    report = json.loads(read_frame(io.BytesIO(process.stdout)))
    semantic = report['semantic_preview']
    assert 'AArch64 register semantics v45' in semantic
    assert 'w2 = w3 << 3;' in semantic
    assert 'w8 = w9 >> 3;' in semantic
    assert 'w14 = w15 >>> 3;' in semantic
    assert 'w2 = w1 >>> 4; /* constant=0xffffffff proven_by=[0x1000000c4,0x1000000c8] */' in semantic
    assert 'word=531d7062' in semantic
print('ARM64 immediate shifts passed through worker analysis')

with tempfile.TemporaryDirectory() as directory:
    source = pathlib.Path(directory) / 'multiply.macho'
    data = image(words=(0x1b027c23, 0x1b020c20, 0x1b028c20))
    source.write_bytes(data)
    process = subprocess.run(
        [sys.argv[1], '--analyze', str(source), hashlib.sha256(data).hexdigest(),
         str(len(data)), str(uuid.uuid4())],
        capture_output=True, timeout=120, check=True,
    )
    report = json.loads(read_frame(io.BytesIO(process.stdout)))
    semantic = report['semantic_preview']
    assert 'AArch64 register semantics v45' in semantic
    assert 'w3 = w1 * w2;' in semantic
    assert 'w0 = w1 * w2 + w3;' in semantic
    assert 'w0 = w3 - w1 * w2;' in semantic
    assert 'word=1b027c23' in semantic
print('ARM64 multiply-add passed through worker analysis')

with tempfile.TemporaryDirectory() as directory:
    source = pathlib.Path(directory) / 'extends.macho'
    data = image(words=(0x52801001, 0x13001c22, 0x53001c23, 0x93407ca6))
    source.write_bytes(data)
    process = subprocess.run(
        [sys.argv[1], '--analyze', str(source), hashlib.sha256(data).hexdigest(),
         str(len(data)), str(uuid.uuid4())],
        capture_output=True, timeout=120, check=True,
    )
    report = json.loads(read_frame(io.BytesIO(process.stdout)))
    semantic = report['semantic_preview']
    assert 'AArch64 register semantics v45' in semantic
    assert 'w2 = sext(w1, 8);' in semantic
    assert 'w3 = zext(w1, 8);' in semantic
    assert 'x6 = sext(' in semantic
    assert 'word=13001c22' in semantic
print('ARM64 sign/zero extends passed through worker analysis')

with tempfile.TemporaryDirectory() as directory:
    source = pathlib.Path(directory) / 'shifted-register.macho'
    data = image(words=(0x528000a1, 0x52800062, 0x0b020820, 0x0a220c20))
    source.write_bytes(data)
    process = subprocess.run(
        [sys.argv[1], '--analyze', str(source), hashlib.sha256(data).hexdigest(),
         str(len(data)), str(uuid.uuid4())],
        capture_output=True, timeout=120, check=True,
    )
    report = json.loads(read_frame(io.BytesIO(process.stdout)))
    semantic = report['semantic_preview']
    assert 'AArch64 register semantics v45' in semantic
    assert 'w0 = w1 + (w2 << 2);' in semantic
    assert 'w0 = w1 & ~(w2 << 3);' in semantic
    assert 'word=0b020820' in semantic
print('ARM64 shifted register operands passed through worker analysis')

with tempfile.TemporaryDirectory() as directory:
    source = pathlib.Path(directory) / 'extracts.macho'
    data = image(words=(0x52810001, 0x13042c22, 0x53042c23, 0xb3442dee))
    source.write_bytes(data)
    process = subprocess.run(
        [sys.argv[1], '--analyze', str(source), hashlib.sha256(data).hexdigest(),
         str(len(data)), str(uuid.uuid4())],
        capture_output=True, timeout=120, check=True,
    )
    report = json.loads(read_frame(io.BytesIO(process.stdout)))
    semantic = report['semantic_preview']
    assert 'AArch64 register semantics v45' in semantic
    assert 'w2 = sext(w1 >> 4, 8);' in semantic
    assert 'w3 = (w1 >> 4) & 0xff;' in semantic
    assert 'word=53042c23' in semantic
    # bfxil x14, x15, #4, #8 = 0xb3442dee is now modeled
    assert 'x14 = (x14 & ~0xff) | ((x15 >> 4) & 0xff);' in semantic
print('ARM64 bitfield extracts passed through worker analysis')

with tempfile.TemporaryDirectory() as directory:
    source = pathlib.Path(directory) / 'load-store.macho'
    data = image(words=(0xd2820001, 0xf9000420, 0xf9400422, 0xb9400023, 0xd65f03c0))
    source.write_bytes(data)
    process = subprocess.run(
        [sys.argv[1], '--analyze', str(source), hashlib.sha256(data).hexdigest(),
         str(len(data)), str(uuid.uuid4())],
        capture_output=True, timeout=120, check=True,
    )
    report = json.loads(read_frame(io.BytesIO(process.stdout)))
    semantic = report['semantic_preview']
    assert 'AArch64 register semantics v45' in semantic
    assert '*(x1 + 0x8) = x0;' in semantic
    assert 'address=0x1008 proven_by=[0x1000000b8,0x1000000bc]' in semantic
    assert 'x2 = *(x1 + 0x8);' in semantic
    assert 'w3 = *(x1);' in semantic
    assert 'word=b9400023' in semantic
print('ARM64 load/store immediate passed through worker analysis')

with tempfile.TemporaryDirectory() as directory:
    source = pathlib.Path(directory) / 'load-store-subwidth.macho'
    data = image(words=(0xd2820001, 0x39001421, 0x39403c22, 0xb9800c23, 0xd65f03c0))
    source.write_bytes(data)
    process = subprocess.run(
        [sys.argv[1], '--analyze', str(source), hashlib.sha256(data).hexdigest(),
         str(len(data)), str(uuid.uuid4())],
        capture_output=True, timeout=120, check=True,
    )
    report = json.loads(read_frame(io.BytesIO(process.stdout)))
    semantic = report['semantic_preview']
    assert 'AArch64 register semantics v45' in semantic
    assert '*(u8 *)(x1 + 0x5) = w1; /* address=0x1005 proven_by=[0x1000000b8,0x1000000bc] */' in semantic
    assert 'w2 = *(u8 *)(x1 + 0xf); /* address=0x100f proven_by=[0x1000000b8,0x1000000c0] */' in semantic
    assert 'x3 = *(s32 *)(x1 + 0xc); /* address=0x100c proven_by=[0x1000000b8,0x1000000c4] */' in semantic
    assert 'word=b9800c23' in semantic
print('ARM64 sub-width and signed load/store passed through worker analysis')

with tempfile.TemporaryDirectory() as directory:
    source = pathlib.Path(directory) / 'exclusive.macho'
    data = image(words=(0xc85f7c20, 0xc8027c20, 0xc8dffc20, 0xc89ffc20, 0xd65f03c0))
    source.write_bytes(data)
    process = subprocess.run(
        [sys.argv[1], '--analyze', str(source), hashlib.sha256(data).hexdigest(),
         str(len(data)), str(uuid.uuid4())],
        capture_output=True, timeout=120, check=True,
    )
    report = json.loads(read_frame(io.BytesIO(process.stdout)))
    semantic = report['semantic_preview']
    assert 'AArch64 register semantics v45' in semantic
    assert 'x0 = exclusive_load_relaxed(x1);' in semantic
    assert 'w2 = exclusive_store_relaxed(x1, x0);' in semantic
    assert 'x0 = atomic_load_acquire(x1);' in semantic
    assert 'atomic_store_release(x1, x0);' in semantic
    assert 'word=c85f7c20' in semantic
print('ARM64 exclusive and acquire/release load/store passed through worker analysis')

with tempfile.TemporaryDirectory() as directory:
    source = pathlib.Path(directory) / 'system.macho'
    data = image(words=(0xd503201f, 0xd5033bbf, 0xd5033fdf, 0xd4000001, 0xd4200040, 0xd65f03c0))
    source.write_bytes(data)
    process = subprocess.run(
        [sys.argv[1], '--analyze', str(source), hashlib.sha256(data).hexdigest(),
         str(len(data)), str(uuid.uuid4())],
        capture_output=True, timeout=120, check=True,
    )
    report = json.loads(read_frame(io.BytesIO(process.stdout)))
    semantic = report['semantic_preview']
    assert 'AArch64 register semantics v45' in semantic
    assert '/* nop */' in semantic
    assert '/* dmb ish */' in semantic
    assert '/* isb */' in semantic
    assert '/* svc(0): exception; value flow stops here */' in semantic
    assert '/* brk(2): exception; value flow stops here */' in semantic
    assert 'word=d5033bbf' in semantic
print('ARM64 barriers, hints and exceptions passed through worker analysis')

with tempfile.TemporaryDirectory() as directory:
    source = pathlib.Path(directory) / 'compare.macho'
    data = image(words=(0xd28000a0, 0xf100141f, 0x91000401, 0xea01001f, 0xb1000c5f, 0xd65f03c0))
    source.write_bytes(data)
    process = subprocess.run(
        [sys.argv[1], '--analyze', str(source), hashlib.sha256(data).hexdigest(),
         str(len(data)), str(uuid.uuid4())],
        capture_output=True, timeout=120, check=True,
    )
    report = json.loads(read_frame(io.BytesIO(process.stdout)))
    semantic = report['semantic_preview']
    assert 'AArch64 register semantics v45' in semantic
    assert '/* cmp x0, 0x5 */' in semantic
    assert '/* tst x0, x1 */' in semantic
    assert '/* cmn x2, 0x3 */' in semantic
    # The compare has no register effect, so the constant folds through it.
    assert 'x1 = x0 + 0x1; /* constant=0x6' in semantic
    assert 'word=f100141f' in semantic
print('ARM64 flag-only compares passed through worker analysis')

with tempfile.TemporaryDirectory() as directory:
    source = pathlib.Path(directory) / 'sysreg.macho'
    data = image(words=(0xd53bd048, 0xd51bd040, 0xd28000e1, 0xd53bd040, 0x91000422, 0xd65f03c0))
    source.write_bytes(data)
    process = subprocess.run(
        [sys.argv[1], '--analyze', str(source), hashlib.sha256(data).hexdigest(),
         str(len(data)), str(uuid.uuid4())],
        capture_output=True, timeout=120, check=True,
    )
    report = json.loads(read_frame(io.BytesIO(process.stdout)))
    semantic = report['semantic_preview']
    assert 'AArch64 register semantics v45' in semantic
    assert 'x8 = mrs(TPIDR_EL0);' in semantic
    assert '/* msr TPIDR_EL0, x0 */' in semantic
    # MRS is a barrier on its destination only; other registers survive.
    assert 'x2 = x1 + 0x1; /* constant=0x8' in semantic
    assert 'word=d53bd048' in semantic
print('ARM64 system register reads and writes passed through worker analysis')

with tempfile.TemporaryDirectory() as directory:
    source = pathlib.Path(directory) / 'lse.macho'
    data = image(words=(0xf8bfc108, 0x38bfc108, 0xf8290108, 0xb8e90108, 0x38bc8108, 0xc8e8fd38, 0xf8ff8108, 0xd65f03c0))
    source.write_bytes(data)
    process = subprocess.run(
        [sys.argv[1], '--analyze', str(source), hashlib.sha256(data).hexdigest(),
         str(len(data)), str(uuid.uuid4())],
        capture_output=True, timeout=120, check=True,
    )
    report = json.loads(read_frame(io.BytesIO(process.stdout)))
    semantic = report['semantic_preview']
    assert 'AArch64 register semantics v45' in semantic
    assert 'x8 = ldapr(x8);' in semantic
    assert 'w8 = ldaprb(x8);' in semantic
    assert 'x8 = ldadd(x8, x9);' in semantic
    assert 'w8 = ldaddal(x8, w9);' in semantic
    assert 'w8 = swpab(x8, w28);' in semantic
    assert '/* casal x8, x24, x9 */' in semantic
    assert 'x8 = swpal(x8, xzr);' in semantic
    assert 'word=f8bfc108' in semantic
print('ARM64 LSE atomics passed through worker analysis')

with tempfile.TemporaryDirectory() as directory:
    source = pathlib.Path(directory) / 'ccmp.macho'
    data = image(words=(0xfa450080, 0xfa430880, 0xba49c880, 0x7a43104f, 0xfa46c0a8, 0xd65f03c0))
    source.write_bytes(data)
    process = subprocess.run(
        [sys.argv[1], '--analyze', str(source), hashlib.sha256(data).hexdigest(),
         str(len(data)), str(uuid.uuid4())],
        capture_output=True, timeout=120, check=True,
    )
    report = json.loads(read_frame(io.BytesIO(process.stdout)))
    semantic = report['semantic_preview']
    assert 'AArch64 register semantics v45' in semantic
    assert '/* ccmp x4, x5, #0x0, eq */' in semantic
    assert '/* ccmp x4, 0x3, #0x0, eq */' in semantic
    assert '/* ccmn x4, 0x9, #0x0, gt */' in semantic
    assert '/* ccmp w2, w3, #0xf, ne */' in semantic
    assert 'word=fa450080' in semantic
print('ARM64 conditional comparisons passed through worker analysis')
