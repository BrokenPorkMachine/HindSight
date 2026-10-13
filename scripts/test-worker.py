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
    assert 'AArch64 register semantics v2' in semantic
    assert 'constant=0x2 proven_by=[0x1000000b8,0x1000000bc]' in semantic
    assert 'ABI result register x0 /* constant=0x2 proven_by=[0x1000000b8,0x1000000bc] */' in semantic
print('ARM64 straight-line constant provenance passed through worker analysis')
