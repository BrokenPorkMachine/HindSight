//! Bounded ELF and PE image admission. Addresses are preferred image addresses,
//! never live process addresses. No loader, relocation application or execution.
use super::*;

fn half(b: &[u8], be: bool) -> u16 {
    let bytes = b[..2].try_into().unwrap();
    if be {
        u16::from_be_bytes(bytes)
    } else {
        u16::from_le_bytes(bytes)
    }
}

fn image(
    format: &'static str,
    arch: Arch,
    bits: u32,
    be: bool,
    o: &Options,
    length: u64,
) -> io::Result<Image> {
    if o.base != 0 {
        return Err(bad(
            "Structured images use header addresses; raw load-address overrides are unsupported",
        ));
    }
    if let Some(selected) = Arch::option(&o.architecture) {
        if selected != arch {
            return Err(bad("Selected architecture disagrees with image machine"));
        }
    }
    Ok(Image {
        format,
        entries: vec![],
        foreign: FormatEvidence::default(),
        arch,
        bits,
        be,
        origin: o.offset,
        length,
        cpu: 0,
        subtype: 0,
        filetype: 0,
        flags: 0,
        sections: vec![],
        commands: vec![],
        code: None,
        encrypted: false,
        inventory: String::new(),
        raw: false,
    })
}

fn section(i: &mut Image, s: Section) -> io::Result<()> {
    if i.sections.len() >= 256 {
        return Err(bad("Image section budget exceeded"));
    }
    add(s.address, s.size)?;
    if !s.zero {
        range(s.offset, s.size, add(i.origin, i.length)?)?;
        if s.offset < i.origin {
            return Err(bad("Section precedes selected image"));
        }
    }
    for other in &i.sections {
        if s.size > 0
            && other.size > 0
            && (s.address < other.address + other.size && other.address < s.address + s.size
                || !s.zero
                    && !other.zero
                    && s.offset < other.offset + other.size
                    && other.offset < s.offset + s.size)
        {
            return Err(bad("Overlapping image sections"));
        }
    }
    if s.executable && s.size > 0 {
        if s.address % i.arch.unit() != 0 || s.size % i.arch.unit() != 0 {
            return Err(bad("Unaligned instruction section"));
        }
        if i.code.is_none() || s.name == ".text" {
            i.code = Some(i.sections.len());
        }
    }
    i.sections.push(s);
    Ok(())
}

fn entry(i: &mut Image, address: u64) -> io::Result<()> {
    if address == 0 {
        return Ok(());
    }
    if address % i.arch.unit() != 0
        || !i
            .sections
            .iter()
            .any(|s| s.executable && !s.zero && s.contains(address))
    {
        return Err(bad(
            "Entry point is outside file-backed executable mappings",
        ));
    }
    i.entries.push(address);
    // Decode the section containing the entry when no conventional .text exists.
    if !i.sections.iter().any(|s| s.executable && s.name == ".text") {
        i.code = i
            .sections
            .iter()
            .position(|s| s.executable && s.contains(address));
    }
    Ok(())
}

pub(super) fn elf(file: &mut File, o: &Options) -> io::Result<Image> {
    let length = file.metadata()?.len() - o.offset;
    range(0, 16, length)?;
    let ident = read(file, o.offset, 16)?;
    let bits = match ident[4] {
        1 => 32,
        2 => 64,
        _ => return Err(bad("Unsupported ELF class")),
    };
    let be = match ident[5] {
        1 => false,
        2 => true,
        _ => return Err(bad("Invalid ELF byte order")),
    };
    if ident[6] != 1 {
        return Err(bad("Unsupported ELF identification version"));
    }
    let fixed = if bits == 64 { 64 } else { 52 };
    range(0, fixed, length)?;
    let h = read(file, o.offset, fixed as usize)?;
    let machine = half(&h[18..], be);
    let arch = match (machine, bits) {
        (3, 32) => Arch::X86,
        (62, 64) => Arch::X64,
        (183, 64) => Arch::A64,
        _ => {
            return Err(bad(
                "ELF machine/class is unsupported; supported: i386, x86-64, AArch64",
            ))
        }
    };
    let mut i = image("ELF", arch, bits, be, o, length)?;
    i.cpu = machine as u32;
    i.filetype = half(&h[16..], be) as u32;
    if !matches!(i.filetype, 2 | 3) {
        return Err(bad("ELF admission requires an executable or shared image; relocatable/core files are unsupported"));
    }
    if u32v(&h[20..], be) != 1 {
        return Err(bad("Unsupported ELF header version"));
    }
    let (address, phoff, shoff, flags, tail) = if bits == 64 {
        (
            u64v(&h[24..], be),
            u64v(&h[32..], be),
            u64v(&h[40..], be),
            u32v(&h[48..], be),
            52,
        )
    } else {
        (
            u32v(&h[24..], be) as u64,
            u32v(&h[28..], be) as u64,
            u32v(&h[32..], be) as u64,
            u32v(&h[36..], be),
            40,
        )
    };
    i.flags = flags;
    let ehsize = half(&h[tail..], be) as u64;
    let phsize = half(&h[tail + 2..], be) as u64;
    let phnum = half(&h[tail + 4..], be) as u64;
    let shsize = half(&h[tail + 6..], be) as u64;
    let shnum = half(&h[tail + 8..], be) as u64;
    let names_index = half(&h[tail + 10..], be) as u64;
    if ehsize != fixed || phnum == 0 || phnum > 256 || phsize != if bits == 64 { 56 } else { 32 } {
        return Err(bad("Invalid or over-budget ELF program header table"));
    }
    range(phoff, phnum * phsize, length)?;
    let table = read(file, add(o.offset, phoff)?, (phnum * phsize) as usize)?;
    let mut loads = vec![];
    for p in table.chunks_exact(phsize as usize) {
        let kind = u32v(p, be);
        let (offset, va, filesz, memsz, permission, alignment) = if bits == 64 {
            (
                u64v(&p[8..], be),
                u64v(&p[16..], be),
                u64v(&p[32..], be),
                u64v(&p[40..], be),
                u32v(&p[4..], be),
                u64v(&p[48..], be),
            )
        } else {
            (
                u32v(&p[4..], be) as u64,
                u32v(&p[8..], be) as u64,
                u32v(&p[16..], be) as u64,
                u32v(&p[20..], be) as u64,
                u32v(&p[24..], be),
                u32v(&p[28..], be) as u64,
            )
        };
        range(offset, filesz, length)?;
        add(va, memsz)?;
        if kind != 1 {
            continue;
        }
        if filesz > memsz
            || alignment > 1
                && (!alignment.is_power_of_two() || va % alignment != offset % alignment)
        {
            return Err(bad("Invalid ELF load segment extent/alignment"));
        }
        for &(a, m, _, _, _) in &loads {
            if memsz > 0 && m > 0 && va < a + m && a < va + memsz {
                return Err(bad("Overlapping ELF load mappings"));
            }
        }
        loads.push((va, memsz, offset, filesz, permission));
        push(&mut i.inventory, &format!("ELF PT_LOAD VA={va:#x} memory={memsz:#x} file={offset:#x} bytes={filesz:#x} permissions={permission:#x}\n"));
    }
    if loads.is_empty() {
        return Err(bad("ELF contains no load mappings"));
    }
    if shoff != 0 {
        if shnum == 0
            || shnum > 256
            || shsize != if bits == 64 { 64 } else { 40 }
            || names_index >= shnum
        {
            return Err(bad("Invalid/extended or over-budget ELF section table"));
        }
        range(shoff, shnum * shsize, length)?;
        let headers = read(file, add(o.offset, shoff)?, (shnum * shsize) as usize)?;
        let fields = |s: &[u8]| -> (u64, u64, u64, u64) {
            if bits == 64 {
                (
                    u64v(&s[8..], be),
                    u64v(&s[16..], be),
                    u64v(&s[24..], be),
                    u64v(&s[32..], be),
                )
            } else {
                (
                    u32v(&s[8..], be) as u64,
                    u32v(&s[12..], be) as u64,
                    u32v(&s[16..], be) as u64,
                    u32v(&s[20..], be) as u64,
                )
            }
        };
        let mut names = vec![];
        if names_index != 0 {
            let s = &headers[(names_index * shsize) as usize..];
            let (_, _, offset, size) = fields(s);
            if u32v(&s[4..], be) != 3 || size > SCAN as u64 {
                return Err(bad("Invalid ELF section-name table"));
            }
            range(offset, size, length)?;
            names = read(file, add(o.offset, offset)?, size as usize)?;
        }
        for s in headers.chunks_exact(shsize as usize).skip(1) {
            let (flags, va, offset, size) = fields(s);
            let zero = u32v(&s[4..], be) == 8;
            if !zero {
                range(offset, size, length)?;
            }
            if flags & 2 == 0 || size == 0 {
                continue;
            }
            let executable = flags & 4 != 0;
            if !loads.iter().any(|&(a, m, f, n, p)| {
                va >= a
                    && size <= m.saturating_sub(va - a)
                    && (zero
                        || offset >= f
                            && size <= n.saturating_sub(offset - f)
                            && va - a == offset - f)
                    && (!executable || p & 1 != 0)
            }) {
                return Err(bad("ELF section disagrees with load mappings"));
            }
            let index = u32v(s, be) as usize;
            let label = if names.is_empty() {
                format!("section_{}", i.sections.len())
            } else {
                if index >= names.len() || !names[index..].contains(&0) {
                    return Err(bad("Invalid ELF section name"));
                }
                name(&names[index..names.len().min(index + 256)])
            };
            section(
                &mut i,
                Section {
                    name: label,
                    segment: "ELF".into(),
                    address: va,
                    size,
                    offset: add(o.offset, offset)?,
                    executable: executable && !zero,
                    zero,
                    ..Section::default()
                },
            )?;
        }
        elf_symbols(file, &mut i, &headers, shsize as usize)?;
    } else if shnum != 0 || names_index != 0 {
        return Err(bad("ELF section count without section table"));
    }
    if i.sections.is_empty() {
        for (index, &(va, memsz, offset, filesz, permission)) in loads.iter().enumerate() {
            if filesz > 0 {
                section(
                    &mut i,
                    Section {
                        name: format!("LOAD{index}"),
                        segment: "ELF".into(),
                        address: va,
                        size: filesz,
                        offset: add(o.offset, offset)?,
                        executable: permission & 1 != 0,
                        ..Section::default()
                    },
                )?;
            }
            if memsz > filesz {
                section(
                    &mut i,
                    Section {
                        name: format!("LOAD{index}.bss"),
                        segment: "ELF".into(),
                        address: add(va, filesz)?,
                        size: memsz - filesz,
                        zero: true,
                        ..Section::default()
                    },
                )?;
            }
        }
    }
    entry(&mut i, address)?;
    push(&mut i.inventory, "ELF static structural analysis with section-backed symbol tables; sectionless dynamic symbols, runtime linking, relocations, Linux native attach and semantic lifting remain unavailable. PIE addresses are preferred addresses without a runtime slide.\n");
    Ok(i)
}

fn elf_symbols(file: &mut File, i: &mut Image, headers: &[u8], stride: usize) -> io::Result<()> {
    let wide = i.bits == 64;
    let be = i.be;
    let mut budget = 16 * 1024 * 1024usize;
    let mut names_budget = 1024 * 1024usize;
    let count = headers.len() / stride;
    let extent = |s: &[u8]| -> (u64, u64) {
        if wide {
            (u64v(&s[24..], be), u64v(&s[32..], be))
        } else {
            (u32v(&s[16..], be) as u64, u32v(&s[20..], be) as u64)
        }
    };
    for s in headers.chunks_exact(stride) {
        let kind = u32v(&s[4..], be);
        if !matches!(kind, 2 | 11) {
            continue;
        }
        let (offset, size) = extent(s);
        let link = u32v(&s[if wide { 40 } else { 24 }..], be) as usize;
        let entry_size = if wide {
            u64v(&s[56..], be)
        } else {
            u32v(&s[36..], be) as u64
        };
        let expected = if wide { 24 } else { 16 };
        if entry_size != expected
            || size % expected != 0
            || size / expected > 200_000
            || link == 0
            || link >= count
        {
            return Err(bad("Invalid or over-budget ELF symbol table"));
        }
        let strings_header = &headers[link * stride..];
        if u32v(&strings_header[4..], be) != 3 {
            return Err(bad("ELF symbols do not link to a string table"));
        }
        let (strings_offset, strings_size) = extent(strings_header);
        if strings_size > 8 * 1024 * 1024
            || size + strings_size > budget as u64
            || i.foreign.symbol_count + (size / expected) as usize > 200_000
        {
            return Err(bad("ELF symbol/string byte or record budget exceeded"));
        }
        budget -= (size + strings_size) as usize;
        range(offset, size, i.length)?;
        range(strings_offset, strings_size, i.length)?;
        let strings = read(file, add(i.origin, strings_offset)?, strings_size as usize)?;
        if strings.first() != Some(&0) || strings.last() != Some(&0) {
            return Err(bad("Invalid ELF symbol string-table framing"));
        }
        let table = read(file, add(i.origin, offset)?, size as usize)?;
        let source = if kind == 11 {
            "ELF dynamic-symbol"
        } else {
            "ELF static-symbol"
        };
        for record in table.chunks_exact(expected as usize) {
            i.foreign.symbol_count += 1;
            let index = u32v(record, be) as usize;
            let (value, symbol_size, info, visibility, section_index) = if wide {
                (
                    u64v(&record[8..], be),
                    u64v(&record[16..], be),
                    record[4],
                    record[5] & 3,
                    half(&record[6..], be) as usize,
                )
            } else {
                (
                    u32v(&record[4..], be) as u64,
                    u32v(&record[8..], be) as u64,
                    record[12],
                    record[13] & 3,
                    half(&record[14..], be) as usize,
                )
            };
            if section_index == 0xffff {
                return Err(bad("Extended ELF symbol section indexes are unsupported"));
            }
            if section_index != 0 && section_index < 0xff00 && section_index >= count {
                return Err(bad("ELF symbol section index outside table"));
            }
            if index >= strings.len() {
                return Err(bad("ELF symbol name outside string table"));
            }
            if index == 0 {
                continue;
            }
            let tail = &strings[index..strings.len().min(index + 1025)];
            let end = tail
                .iter()
                .position(|&b| b == 0)
                .ok_or_else(|| bad("Unterminated or over-budget ELF symbol name"))?;
            if end > names_budget {
                return Err(bad("ELF symbol name budget exceeded"));
            }
            names_budget -= end;
            let label = name(&tail[..end]);
            if label.is_empty() {
                continue;
            }
            let symbol_type = info & 15;
            let binding = info >> 4;
            if section_index == 0 {
                if matches!(binding, 1 | 2) {
                    push(&mut i.foreign.imports, &format!("name={label} undefined source={source} type={symbol_type} binding={binding} visibility={visibility}; runtime target unresolved\n"));
                }
                continue;
            }
            let mut proven_address = false;
            if section_index < count {
                let target = &headers[section_index * stride..];
                let (flags, va) = if wide {
                    (u64v(&target[8..], be), u64v(&target[16..], be))
                } else {
                    (
                        u32v(&target[8..], be) as u64,
                        u32v(&target[12..], be) as u64,
                    )
                };
                let (_, section_size) = extent(target);
                // TLS values are offsets, not preferred addresses.
                if symbol_type != 6 && flags & 2 != 0 && value >= va && value - va < section_size {
                    if symbol_size > section_size - (value - va) {
                        return Err(bad("ELF symbol extent exceeds its section"));
                    }
                    proven_address = true;
                    if symbol_type == 2
                        && flags & 4 != 0
                        && value % i.arch.unit() == 0
                        && i.sections.iter().any(|s| s.executable && s.contains(value))
                    {
                        i.foreign
                            .functions
                            .push((value, label.clone(), source.into()));
                    }
                } else if symbol_type == 2 && flags & 2 != 0 {
                    return Err(bad("ELF function symbol lies outside its declared section"));
                }
            }
            let row = if proven_address {
                format!("0x{value:016x} name={label} source={source} type={symbol_type} binding={binding} visibility={visibility} section={section_index}\n")
            } else {
                format!("name={label} value={value:#x} source={source} type={symbol_type} binding={binding} section={section_index}; address unresolved\n")
            };
            push(&mut i.foreign.symbols, &row);
            if kind == 11 && matches!(binding, 1 | 2) && matches!(visibility, 0 | 3) {
                push(&mut i.foreign.exports, &row);
            }
        }
    }
    push(&mut i.inventory, &format!("ELF symbol records={} (including unnamed records); undefined names remain address-free.\n", i.foreign.symbol_count));
    Ok(())
}

pub(super) fn pe(file: &mut File, o: &Options) -> io::Result<Image> {
    let length = file.metadata()?.len() - o.offset;
    range(0, 64, length)?;
    let dos = read(file, o.offset, 64)?;
    let nt = u32v(&dos[60..], false) as u64;
    if nt < 64 {
        return Err(bad("PE header overlaps DOS header"));
    }
    range(nt, 24, length)?;
    let h = read(file, add(o.offset, nt)?, 24)?;
    if &h[..4] != b"PE\0\0" {
        return Err(bad("Missing PE signature"));
    }
    let machine = half(&h[4..], false);
    let count = half(&h[6..], false) as u64;
    let optional_size = half(&h[20..], false) as u64;
    if count == 0 || count > 96 || !(96..=4096).contains(&optional_size) {
        return Err(bad("Invalid/over-budget PE header table"));
    }
    let opt = add(nt, 24)?;
    range(opt, optional_size, length)?;
    let p = read(file, add(o.offset, opt)?, optional_size as usize)?;
    let bits = match half(&p, false) {
        0x10b => 32,
        0x20b => 64,
        _ => return Err(bad("Unsupported PE optional header")),
    };
    let arch = match (machine, bits) {
        (0x14c, 32) => Arch::X86,
        (0x8664, 64) => Arch::X64,
        (0xaa64, 64) => Arch::A64,
        _ => return Err(bad("Unsupported PE machine/header width")),
    };
    let mut i = image("PE/COFF", arch, bits, false, o, length)?;
    i.cpu = machine as u32;
    i.flags = half(&h[22..], false) as u32;
    let base = if bits == 64 {
        u64v(&p[24..], false)
    } else {
        u32v(&p[28..], false) as u64
    };
    let directory_offset = if bits == 64 { 112 } else { 96 };
    if optional_size < directory_offset {
        return Err(bad("Truncated PE optional header"));
    }
    let directories = u32v(&p[directory_offset as usize - 4..], false) as u64;
    if directories > 16 || directory_offset + directories * 8 > optional_size {
        return Err(bad("Invalid PE data-directory extent"));
    }
    let image_size = u32v(&p[56..], false) as u64;
    let headers_size = u32v(&p[60..], false) as u64;
    let section_alignment = u32v(&p[32..], false) as u64;
    let file_alignment = u32v(&p[36..], false) as u64;
    if !section_alignment.is_power_of_two()
        || !file_alignment.is_power_of_two()
        || section_alignment < file_alignment
        || (section_alignment < 4096 && section_alignment != file_alignment)
        || (section_alignment >= 4096 && !(512..=65536).contains(&file_alignment))
    {
        return Err(bad("Invalid PE file/section alignment"));
    }
    add(base, image_size)?;
    range(0, headers_size, length)?;
    let sections_offset = add(opt, optional_size)?;
    range(sections_offset, count * 40, headers_size)?;
    if headers_size > image_size {
        return Err(bad("PE headers exceed image extent"));
    }
    let table = read(file, add(o.offset, sections_offset)?, (count * 40) as usize)?;
    let mut extents = vec![];
    for s in table.chunks_exact(40) {
        let virtual_size = u32v(&s[8..], false) as u64;
        let rva = u32v(&s[12..], false) as u64;
        let raw_size = u32v(&s[16..], false) as u64;
        let offset = u32v(&s[20..], false) as u64;
        let flags = u32v(&s[36..], false);
        let memory_size = virtual_size.max(raw_size);
        range(rva, memory_size, image_size)?;
        range(offset, raw_size, length)?;
        if rva < headers_size
            || rva % section_alignment != 0
            || raw_size > 0
                && (offset < headers_size
                    || offset % file_alignment != 0
                    || raw_size % file_alignment != 0)
        {
            return Err(bad("Invalid PE section extent/alignment"));
        }
        for &(a, n, f, z) in &extents {
            if memory_size > 0 && n > 0 && rva < a + n && a < rva + memory_size
                || raw_size > 0 && z > 0 && offset < f + z && f < offset + raw_size
            {
                return Err(bad("Overlapping PE section mappings"));
            }
        }
        extents.push((rva, memory_size, offset, raw_size));
        let label = name(&s[..8]);
        push(&mut i.inventory, &format!("PE section {label} RVA={rva:#x} virtual={virtual_size:#x} raw={raw_size:#x} characteristics={flags:#x}\n"));
        // File alignment padding is not instruction evidence.
        let backed = if virtual_size == 0 {
            raw_size
        } else {
            raw_size.min(virtual_size)
        };
        if backed > 0 {
            section(
                &mut i,
                Section {
                    name: label.clone(),
                    segment: "PE".into(),
                    address: add(base, rva)?,
                    size: backed,
                    offset: add(o.offset, offset)?,
                    executable: flags & 0x20000000 != 0,
                    ..Section::default()
                },
            )?;
        }
        if virtual_size > backed {
            section(
                &mut i,
                Section {
                    name: format!("{label}.bss"),
                    segment: "PE".into(),
                    address: add(add(base, rva)?, backed)?,
                    size: virtual_size - backed,
                    zero: true,
                    ..Section::default()
                },
            )?;
        }
    }
    let rva = u32v(&p[16..], false) as u64;
    if rva != 0 {
        entry(&mut i, add(base, rva)?)?;
    }
    push(&mut i.inventory, &format!("PE preferred image base={base:#x}; image extent={image_size:#x}; data directories={directories}.\nPE static structural analysis; imports/exports, PDB, relocations, Authenticode validation, Windows execution and semantic lifting remain unavailable. Standalone COFF objects are unsupported.\n"));
    Ok(i)
}
