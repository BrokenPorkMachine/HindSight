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

def emulate_c167(path, entry=0, steps=4, stack=0x10000, breakpoint='none', watchpoint='none', ram='none'):
    identity=str(uuid.uuid4())
    result=subprocess.run([str(WORKER),'--emulate-c167',str(path),str(entry),str(steps),str(stack),str(breakpoint),str(watchpoint),ram,identity],capture_output=True,timeout=120,check=True)
    length=struct.unpack('>I',result.stdout[:4])[0]
    assert len(result.stdout)==length+4,result.stderr
    reply=json.loads(result.stdout[4:])
    assert reply['kind']=='c167_emulation_result' and reply['request_id']==identity
    return reply

def image(cpu=7, code=b'\x55\x89\xe5\xc3', *, be=False, extra=b'', thumb_symbol=False):
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
        symbol=struct.pack(endian+'IBBHQ',1,0xf,1,8 if thumb_symbol else 0,0x1000+codeoff)
    else:
        segment=struct.pack(endian+'II16s8I',1,seg+3*sec,b'__TEXT',0x1000,size,0,size,7,7,3,0)
        def section(name,va,n,off,flags):return struct.pack(endian+'16s16s9I',name,b'__TEXT',va,n,off,2,0,0,flags,0,0)
        symbol=struct.pack(endian+'IBBHI',1,0xf,1,8 if thumb_symbol else 0,0x1000+codeoff)
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
        data,off=image(cpu,code,thumb_symbol=architecture=='thumb');path=root/(architecture+'.macho');path.write_bytes(data)
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
        if architecture in ('i386', 'x86_64'):
            assert 'semantic=stack-operation' in a['annotated_disassembly'], a['annotated_disassembly']
        assert 'prologue candidate' in a['function_intelligence']
        if architecture not in ('arm', 'thumb'):
            assert 'epilogue candidate' in a['function_intelligence'], architecture
        assert 'validated executable-target run count=3' in a['jump_tables']
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
    mixed_data,_=image(12,bytes.fromhex('80b5694680bd00bf'),thumb_symbol=True)
    mixed_path=root/'mixed-arm-thumb';mixed_path.write_bytes(mixed_data)
    mixed=invoke('analyze',mixed_path,options=('macho','arm','0','0'))
    assert mixed['kind']=='analysis_result',mixed
    assert 'Mixed ARM/Thumb decode: 1 metadata-marked Thumb range' in mixed['metadata'],mixed
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
    first,_=image(12,bytes.fromhex('80b56f4680bd'),thumb_symbol=True)
    second,_=image(12,bytes.fromhex('10b5044600bf10bd'),thumb_symbol=True)
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
    # Encoded firmware is inventoried without pretending that its payload is
    # decoded. The capture remains useful for triage, while page requests stay
    # blocked until a plaintext image is supplied.
    path=root/'firmware.img4'
    path.write_bytes(b'\x30\x82\x00\x20IMG4' + bytes(0x80) + b'IM4P' + bytes(0x40) + b'bvx2' + bytes(0x20))
    container=invoke('capture',path,options=('auto','auto','0','0'),destination=root/'firmware.capture')
    assert container['file_kind']=='IMG4 firmware container',container
    assert 'IMG4 marker offsets' in container['structure'] and 'IM4P payload marker offsets' in container['structure'],container
    assert ('bvx2 compression marker offsets' in container['structure']
            and 'DER-like prefix' in container['structure']
            and 'ASN.1 structure was not accepted' in container['structure']),container
    assert container['disassembly_status']=='unavailable' and 'plaintext payload' in container['disassembly'],container
    assert invoke('page',path,options=('auto','auto','0','0'))['kind']=='error'
    # ECU firmware: ME7-class containers validate through their C167 vector table,
    # the checksum descriptor table is parsed and verified at the fixed offset, and
    # non-ME7 dumps are rejected with an actionable error instead of being decoded
    # as anonymous raw code.
    def ecu_image(size=0x20000, vectors=8, bad_entry=False, tail=bytes([0x44]*4+[0x83,0x83])):
        data=bytearray(size)
        for v in range(vectors):
            data[4*v:4*v+4]=struct.pack('<BBH',0xEA,0x00,0x0200+8*v)
        data[0x200:0x204]=struct.pack('<BBH',0xCA,0x00,0x0300)
        data[0x204:0x208]=struct.pack('<BBH',0xEA,0x00,0x0200)
        data[0x208:0x20A]=b'\xCB\x00'
        data[0x2FE:0x300]=b'\xDB\x00'
        data[0x300:0x312]=bytes.fromhex('98 24 a8 34 00 a2 10 b3 26 fa ff ff 36 fb ff ff 3d 08')
        def wordsum(lo,hi):
            total=0
            for a in range(lo,hi,2):
                total+=data[a]|(data[a+1]<<8)
            return total&0xFFFFFFFF
        entries=[(0x0000,0x01FF)]+([(0x0200,0x027F)] if bad_entry else [])
        for i,(lo,hi) in enumerate(entries):
            total=wordsum(lo,hi)
            stored=total if not (bad_entry and i==1) else total^0xFF
            # Keep the descriptor pair internally consistent so this fixture
            # reaches the stored-vs-computed checksum validation path.
            struct.pack_into('<IIII',data,0x1F810+16*i,lo,hi,stored,~stored&0xFFFFFFFF)
        data[0x0F000:0x0F00C]=bytes([0x99,0x99,0x99,0x00,0x0A,0x50,0x57,0x52,0x48,0x4A,0x31,0x32])
        data[size-6:]=tail
        return bytes(data)
    ecu_path=root/'ecu';ecu_path.write_bytes(ecu_image())
    emu_path=root/'c167-emu';emu_path.write_bytes(bytes([0xE0,0x51,0x08,0x13,0xF0,0x21,0x44,0x00]))
    emu=emulate_c167(emu_path,steps=3)
    assert emu['stop']=='Budget' and emu['registers']['r1']==8 and emu['registers']['r2']==8 and len(emu['trace'])==3,emu
    stopped=emulate_c167(emu_path,steps=10,breakpoint=2)
    assert stopped['stop']=='Breakpoint(2)' and stopped['pc']==2 and stopped['steps']==1,stopped
    watch_path=root/'c167-watch';watch_path.write_bytes(bytes([0xE0,0x51,0xF6,0xF1,0x20,0x00])+bytes(0x3A))
    watched=emulate_c167(watch_path,steps=10,watchpoint=0x21)
    assert watched['stop']=='Watchpoint { address: 33, access: Write }' and watched['steps']==2,watched
    sfr_path=root/'c167-sfr';sfr_path.write_bytes(bytes([
        0xE6,0x08,0x34,0xF9,0xE6,0x09,0x80,0x00,
        0xE6,0x06,0x34,0x12,0xE6,0x07,0x78,0x56]))
    sfr=emulate_c167(sfr_path,steps=4,stack=0x100)
    assert (sfr['cp'],sfr['sp'],sfr['mdh'],sfr['mdl'])==(0xF800,0x80,0x1234,0x5678),sfr
    md_path=root/'c167-md';md_path.write_bytes(bytes([
        0xE6,0xF1,0x03,0x00,0xE6,0xF2,0x04,0x00,0x1B,0x12,
        0xE6,0xF3,0x02,0x00,0x7B,0x33]))
    md=emulate_c167(md_path,steps=5,stack=0x100)
    assert (md['mdh'],md['mdl'],md['steps'])==(0,6,5),md
    fault_path=root/'c167-div-zero';fault_path.write_bytes(bytes([0x5B,0x11]))
    fault=emulate_c167(fault_path,steps=1,stack=0x100)
    assert fault['stop']=='ArithmeticFault { pc: 0, reason: "division by zero" }' and fault['steps']==0,fault
    ram_path=root/'c167-ram';ram_path.write_bytes(bytes([
        0xF2,0xF1,0x40,0x00,0x08,0x11,0xF6,0xF1,0x42,0x00,0xF2,0xF2,0x42,0x00])+bytes(0x40))
    ram=emulate_c167(ram_path,steps=4,stack=0x100,ram='0x40=3412')
    assert ram['registers']['r2']==0x1235 and ram['ram_seed_bytes']==2 and ram['ram_write_bytes']==2,ram
    assert ram['ram']['000040']==0x34 and ram['ram']['000042']==0x35,ram
    captured_ecu=root/'ecu.capture'
    ecu_capture=invoke('capture',ecu_path,options=('ecu','auto','0','0'),destination=captured_ecu)
    assert ecu_capture['kind']=='header_result',ecu_capture
    assert ecu_capture['file_kind']=='ecu raw image',ecu_capture
    assert 'ME7-class ECU flash dump' in ecu_capture['structure'] and '8 consecutive JMPA vectors' in ecu_capture['structure'],ecu_capture
    ecu_page=invoke('page',ecu_path,options=('ecu','auto','0','0'))
    assert ecu_page['kind']=='page_result' and ecu_page['unit_bytes']==2 and 'jmpa cc_UC' in ecu_page['text'],ecu_page
    ecu_analysis=invoke('analyze',ecu_path,options=('ecu','auto','0','0'))
    assert ecu_analysis['kind']=='analysis_result' and set(ecu_analysis)==REPORT_FIELDS
    assert 'ecu vector table: 8 consecutive JMPA vectors' in ecu_analysis['metadata'],ecu_analysis
    assert 'direct-call candidate' in ecu_analysis['functions'],ecu_analysis
    assert 'verified sum' in ecu_analysis['metadata'] and 'INVALID stored' not in ecu_analysis['metadata'],ecu_analysis
    assert 'ecu checksummed region candidate' in ecu_analysis['metadata'],ecu_analysis
    assert 'ecu multipoint checksum code signature: exact fixed-byte match' in ecu_analysis['metadata'],ecu_analysis
    assert 'containing-function entry candidate 0x300' in ecu_analysis['metadata'],ecu_analysis
    assert 'ecu identifier candidate: 505752484a12' in ecu_analysis['metadata'],ecu_analysis
    ecu_bad=root/'ecu-bad';ecu_bad.write_bytes(ecu_image(bad_entry=True))
    ecu_bad_analysis=invoke('analyze',ecu_bad,options=('ecu','auto','0','0'))
    assert 'INVALID stored' in ecu_bad_analysis['metadata'] and '1 invalid' in ecu_bad_analysis['metadata'],ecu_bad_analysis
    ecu_logical=root/'ecu-logical';ecu_logical.write_bytes(ecu_image(tail=b'\x44'*16))
    ecu_logical_analysis=invoke('analyze',ecu_logical,options=('ecu','auto','0','0'))
    assert '0x44 fill' in ecu_logical_analysis['metadata'],ecu_logical_analysis
    ecu_short=root/'ecu-short';ecu_short.write_bytes(bytes(0x10000))
    assert invoke('analyze',ecu_short,options=('ecu','auto','0','0'))['kind']=='error'
    ecu_plain=root/'ecu-plain';ecu_plain.write_bytes(bytes(0x20000))
    assert invoke('analyze',ecu_plain,options=('ecu','auto','0','0'))['kind']=='error'
    assert invoke('analyze',ecu_path,options=('ecu','auto','0','0x100'))['kind']=='error'
    assert invoke('analyze',ecu_path,options=('ecu','x86_64','0','0'))['kind']=='error'
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
