#!/usr/bin/env python3
"""End-to-end static imports; synthetic bytes only, no Apple firmware shipped."""
import hashlib, json, pathlib, re, struct, subprocess, sys, tempfile, uuid
WORKER = pathlib.Path(sys.argv[1]).resolve()
ROOT = pathlib.Path(__file__).resolve().parents[1]
REPORT_FIELDS = set(re.findall(r'"([a-z_0-9]+)"', (ROOT/'Shared/WorkbenchMessages.swift').read_text().split('static let jsonFields: Set<String> = [',1)[1].split(']',1)[0]))

def invoke(command, path, *, options=('macho','auto','0','0'), start=0, destination=None):
    identity = str(uuid.uuid4()); data = path.read_bytes()
    if command=='capture':
        args = ['--capture-image',str(path),str(destination),identity]
    else:
        args = ['--'+command+'-image',str(path),hashlib.sha256(data).hexdigest(),str(len(data))]
        if command=='page': args += [str(start)]
        args += [identity]
    result = subprocess.run([str(WORKER),*args,*options],capture_output=True,timeout=120,check=True)
    assert len(result.stdout)>=4,result.stderr
    length = struct.unpack('>I',result.stdout[:4])[0]
    assert len(result.stdout)==length+4
    reply = json.loads(result.stdout[4:])
    if reply['kind']!='error':
        assert reply['request_id']==identity
        assert reply['source_sha256']==hashlib.sha256(data).hexdigest()
    return reply

def image(cpu=7, code=b'\x55\x89\xe5\xc3', *, be=False, extra=b''):
    wide=cpu in (0x01000007,0x0100000c); bits=64 if wide else 32
    endian='>' if be else '<'; header=32 if wide else 28
    seg=72 if wide else 56; sec=80 if wide else 68
    commands=seg+3*sec+24+len(extra)
    codeoff=header+commands
    # All fixed-width test streams and pointers are naturally aligned.
    assert codeoff%4==0
    dataoff=codeoff+len(code)
    dataoff=(dataoff+7)&~7
    pointer_width=8 if wide else 4
    pointers=struct.pack(endian+('Q' if wide else 'I')*3,*([0x1000+codeoff]*3))
    payload=b'fixture string\0'+b'W\0i\0d\0e\0\0\0'+ '漢字仮名'.encode('utf-16-le') + b'\0\0'
    const=pointers+payload
    symoff=dataoff+len(const); stroff=symoff+(16 if wide else 12)
    names=b'\0_fixture_function\0'
    size=stroff+len(names)
    nc=2;cursor=0
    while cursor<len(extra): nc+=1;cursor+=struct.unpack_from(endian+'I',extra,cursor+4)[0]
    hdr=struct.pack(endian+('8I' if wide else '7I'),0xfeedfacf if wide else 0xfeedface,cpu,9 if cpu==12 else 3,2,nc,commands,0,*([0] if wide else []))
    if wide:
        segment=struct.pack(endian+'II16sQQQQIIII',0x19,seg+3*sec,b'__TEXT',0x1000,size,0,size,7,7,3,0)
        def section(name,va,n,off,flags):return struct.pack(endian+'16s16sQQIIIIIIII',name,b'__TEXT',va,n,off,2,0,0,flags,0,0,0)
        symbol=struct.pack(endian+'IBBHQ',1,0xf,1,0,0x1000+codeoff)
    else:
        segment=struct.pack(endian+'II16s8I',1,seg+3*sec,b'__TEXT',0x1000,size,0,size,7,7,3,0)
        def section(name,va,n,off,flags):return struct.pack(endian+'16s16s9I',name,b'__TEXT',va,n,off,2,0,0,flags,0,0)
        symbol=struct.pack(endian+'IBBHI',1,0xf,1,8 if cpu==12 else 0,0x1000+codeoff)
    secs=section(b'__text',0x1000+codeoff,len(code),codeoff,0x80000400)+section(b'__const',0x1000+dataoff,len(pointers),dataoff,0)+section(b'__cstring',0x1000+dataoff+len(pointers),len(payload),dataoff+len(pointers),2)
    symcmd=struct.pack(endian+'6I',2,24,symoff,1,stroff,len(names))
    result=hdr+segment+secs+symcmd+extra+code+bytes(dataoff-codeoff-len(code))+const+symbol+names
    assert len(result)==size
    return result,codeoff

with tempfile.TemporaryDirectory() as directory:
    root=pathlib.Path(directory)
    cases=[(7,b'\x55\x89\xe5\xe8\x01\0\0\0\x90\xc3','i386'),
           (0x01000007,b'\x55\x48\x89\xe5\xe8\x01\0\0\0\x90\xc3','x86_64'),
           (0x0100000c,bytes.fromhex('fd7bbfa9fd030091fd7bc1a8c0035fd6'),'aarch64'),
           (12,bytes.fromhex('80b56f4680bd'),'thumb'),
           (12,bytes.fromhex('80402de90070a0e18080bde8'),'arm')]
    for cpu,code,architecture in cases:
        data,off=image(cpu,code);path=root/(architecture+'.macho');path.write_bytes(data)
        options=('macho',architecture,'0','0')
        p=invoke('page',path,options=options)
        assert p['kind']=='page_result',p
        unit=4 if architecture in ('aarch64','arm') else 2 if architecture=='thumb' else 1
        assert p['unit_bytes']==unit and p['word_count']==len(code)//unit
        assert 'ret' in p['text'] or 'pop' in p['text'],p
        if architecture.startswith('x86') or architecture=='i386': assert 'target=0x' in p['text']
        analysis_path=path; analysis_options=options
        if architecture=='aarch64':
            analysis_path=root/'embedded-a64';analysis_path.write_bytes(bytes(0x1000)+data)
            analysis_options=('macho','aarch64','0','0x1000')
        a=invoke('analyze',analysis_path,options=analysis_options)
        assert a['kind']=='analysis_result',a
        assert set(a)==REPORT_FIELDS, set(a)^REPORT_FIELDS
        assert '_fixture_function' in a['functions'] and 'source=nlist' in a['functions']
        assert 'fixture string' in a['strings'] and 'UTF16-LE' in a['strings'] and '漢字仮名' in a['strings']
        assert 'prologue candidate' in a['function_intelligence']
        assert 'epilogue candidate' in a['function_intelligence']
        assert 'pointer-table candidate count=3' in a['jump_tables']
        assert 'Section __TEXT.__const' in a['metadata']
        assert a['cfg'].startswith('block B0000 0x')
        assert len(a['summary'].encode())<32768
        assert all(len(a[k].encode())<65536 for k in REPORT_FIELDS if isinstance(a[k],str) and k!='summary')
        captured=root/(architecture+'.capture')
        c=invoke('capture',path,options=options,destination=captured)
        assert c['kind']=='header_result' and c['disassembly_status']=='preview',c
        assert c['import_options']['architecture']==architecture
        assert captured.read_bytes()==data
        # Reject architecture confusion, corrupt command sizes and range overflow.
        assert invoke('page',path,options=('macho','arm' if cpu!=12 else 'x86_64','0','0'))['kind']=='error'
        bad=bytearray(data);struct.pack_into('<I',bad,32+4 if cpu>=0x1000000 else 28+4,7)
        malformed=root/'malformed';malformed.write_bytes(bad)
        assert invoke('analyze',malformed)['kind']=='error'
    # RIP-relative data/string references resolve through validated section maps.
    data,off=image(0x01000007,b'\x48\x8d\x05'+bytes(4)+b'\xc3')
    dataoff=(off+8+7)&~7
    displacement=dataoff+24-(off+7)
    data,off=image(0x01000007,b'\x48\x8d\x05'+struct.pack('<i',displacement)+b'\xc3')
    path=root/'rip-strings';path.write_bytes(data)
    analysis=invoke('analyze',path)
    assert 'string-reference' in analysis['data_references'] and 'fixture string' in analysis['data_references'],analysis
    # Variable-length paging: Next begins exactly where the prior decode stopped.
    data,off=image(0x01000007,b'\x48\x89\xe5'*200+b'\xc3');path=root/'pages';path.write_bytes(data)
    consumed=0
    while consumed<601:
        page=invoke('page',path,start=consumed)
        assert page['kind']=='page_result',page
        assert page['text'].splitlines()[1].startswith(f'0x{0x1000+off+consumed:016x}')
        count=page['word_count'];assert 0<count<=256
        if consumed+count<601: assert count%3==0
        consumed+=count
    assert invoke('page',path,start=601)['kind']=='error'
    # Embedded SEPI-like inventory and explicit selection are original-file relative.
    first,_=image(12,bytes.fromhex('80b56f4680bd'))
    second,_=image(12,bytes.fromhex('10b5044600bf10bd'))
    path=root/'composite';path.write_bytes(bytes(0x1000)+first+bytes(0x3000-0x1000-len(first))+second)
    c=invoke('capture',path,options=('sepos','auto','0','0'),destination=root/'composite.capture')
    assert 'image-offset=0x1000' in c['structure'] and 'image-offset=0x3000' in c['structure'],c
    page=invoke('page',path,options=('sepos','auto','0','0x3000'))
    assert int(page['section_offset'])>=0x3000
    # Valid metadata-only Mach-O headers remain importable without code.
    path=root/'metadata-only';path.write_bytes(struct.pack('<7I',0xfeedface,7,3,2,0,0,0))
    analysis=invoke('analyze',path);assert analysis['kind']=='analysis_result' and analysis['analyzed_instructions']==0
    assert invoke('page',path)['kind']=='error'
    # Raw firmware needs declared mapping; no filename/magic-free architecture guess.
    path=root/'rom';path.write_bytes(bytes.fromhex('fd7bbfa9fd030091fd7bc1a8c0035fd6'))
    assert invoke('page',path,options=('securerom','auto','0','0'))['kind']=='error'
    page=invoke('page',path,options=('securerom','aarch64','0x100000000','0'))
    assert page['section_address']=='4294967296' and 'ret' in page['text']
    assert invoke('page',path,options=('raw','aarch64','0xfffffffffffffff8','0'))['kind']=='error'
    # Valid universal FAT32/FAT64, plus overlapping slices rejected before selection.
    one,_=image(7);two,_=image(0x01000007,b'\x55\x48\x89\xe5\xc3')
    for wide in [False,True]:
        fmt='>IIQQII' if wide else '>5I'
        def record(cpu,off,n):return struct.pack(fmt,cpu,3,off,n,12,*([0] if wide else []))
        header=struct.pack('>II',0xcafebabf if wide else 0xcafebabe,2)+record(7,0x1000,len(one))+record(0x01000007,0x2000,len(two))
        data=header+bytes(0x1000-len(header))+one+bytes(0x2000-0x1000-len(one))+two
        path=root/'fat';path.write_bytes(data)
        page=invoke('page',path,options=('macho','i386','0','0'));assert page['unit_bytes']==1 and int(page['section_offset'])>=0x1000
        page=invoke('page',path,options=('macho','x86_64','0','0'));assert 'movq' in page['text']
        malformed=bytearray(data);entry=8+(32 if wide else 20);struct.pack_into('>Q' if wide else '>I',malformed,entry+8,0x1000);path.write_bytes(malformed)
        assert invoke('page',path)['kind']=='error'
    # Big endian layouts produce metadata only; never decode as little endian.
    data,_=image(7,be=True);path=root/'big';path.write_bytes(data)
    a=invoke('analyze',path);assert a['kind']=='analysis_result' and a['analyzed_instructions']==0 and 'big endian' in a['metadata']
    assert invoke('page',path)['kind']=='error'
    # Walk function-start ULEBs, entry metadata, export trie and data-in-code.
    def uleb(value):
        out=bytearray()
        while value>=128:out.append((value&127)|128);value>>=7
        out.append(value);return bytes(out)
    extra=struct.pack('<4I',0x26,16,0,0)+struct.pack('<IIQQ',0x80000028,24,0,0)+struct.pack('<4I',0x80000033,16,0,0)+struct.pack('<4I',0x29,16,0,0)
    data,off=image(7,b'\x55\x89\xe5\x90\xc3',extra=extra)
    payload=bytearray(data);cmd=28+56+3*68+24
    starts=uleb(off)+b'\0';startoff=len(payload);payload+=starts
    struct.pack_into('<II',payload,cmd+8,startoff,len(starts))
    struct.pack_into('<Q',payload,cmd+16+8,off)
    root_node=b'\0\x01_exported\0';childoff=len(root_node)+1
    terminal=b'\0'+uleb(off+4)
    trie=root_node+uleb(childoff)+uleb(len(terminal))+terminal+b'\0'
    exportoff=len(payload);payload+=trie;struct.pack_into('<II',payload,cmd+40+8,exportoff,len(trie))
    dice=struct.pack('<IHH',off+3,1,1);diceoff=len(payload);payload+=dice
    struct.pack_into('<II',payload,cmd+56+8,diceoff,len(dice))
    struct.pack_into('<I',payload,28+28,len(payload));struct.pack_into('<I',payload,28+36,len(payload))
    path=root/'metadata-tables';path.write_bytes(payload)
    analysis=invoke('analyze',path)
    assert analysis['kind']=='analysis_result',analysis
    assert 'name=_exported source=export-trie' in analysis['functions']
    assert 'entry-point metadata' in analysis['metadata']
    assert '.data-in-code' in analysis['annotated_disassembly']
    # Malformed child offset must fail within the bounded walk.
    malformed=bytearray(payload);malformed[exportoff+len(root_node)]=127;path.write_bytes(malformed)
    assert invoke('analyze',path)['kind']=='error'
    # Name-table index corruption is an error instead of an invented function name.
    data,_=image(7);bad=bytearray(data);symoff=struct.unpack_from('<I',data,28+56+3*68+8)[0];struct.pack_into('<I',bad,symoff,0xffffffff)
    path=root/'bad-symbol';path.write_bytes(bad);assert invoke('analyze',path)['kind']=='error'
print('Multi-architecture headers, symbols, strings, pointer tables, prologue/epilogue candidates, firmware mappings, fat slices and variable-length pages passed')
