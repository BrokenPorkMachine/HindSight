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
    if !matches!(i.filetype, 1 | 2 | 3) {
        return Err(bad("ELF admission requires a relocatable object, executable, or shared image; core files are unsupported"));
    }
    let relocatable = i.filetype == 1;
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
    if ehsize != fixed
        || if relocatable {
            phnum != 0 || phoff != 0
        } else {
            phnum == 0 || phnum > 256 || phsize != if bits == 64 { 56 } else { 32 }
        }
    {
        return Err(bad("Invalid or over-budget ELF program header table"));
    }
    range(phoff, phnum * phsize, length)?;
    let table = read(file, add(o.offset, phoff)?, (phnum * phsize) as usize)?;
    let mut loads = vec![];
    let mut dynamic_segment: Option<(u64, u64, u64)> = None;
    for p in table.chunks_exact(phsize.max(1) as usize) {
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
        if kind == 2 {
            if dynamic_segment.is_some() {
                return Err(bad("Multiple ELF PT_DYNAMIC segments"));
            }
            dynamic_segment = Some((offset, filesz, va));
        }
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
    if loads.is_empty() && !relocatable {
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
        let mut section_addresses = vec![None; shnum as usize];
        let mut next_object_address = 0x1_0000u64;
        for (section_index, s) in headers.chunks_exact(shsize as usize).enumerate().skip(1) {
            let (flags, header_va, offset, size) = fields(s);
            let zero = u32v(&s[4..], be) == 8;
            if !zero {
                range(offset, size, length)?;
            }
            if flags & 2 == 0 || size == 0 {
                continue;
            }
            let executable = flags & 4 != 0;
            let va = if relocatable {
                let alignment = if bits == 64 { u64v(&s[48..], be) } else { u32v(&s[32..], be) as u64 };
                if alignment > 0x10_0000 || alignment > 1 && !alignment.is_power_of_two() {
                    return Err(bad("Invalid ELF object section alignment"));
                }
                let alignment = alignment.max(i.arch.unit());
                next_object_address = next_object_address.checked_add(alignment - 1).ok_or_else(|| bad("ELF object address overflow"))? & !(alignment - 1);
                let assigned = next_object_address;
                next_object_address = add(assigned, size)?;
                assigned
            } else {
                header_va
            };
            if !relocatable && !loads.iter().any(|&(a, m, f, n, p)| {
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
            section_addresses[section_index] = Some(va);
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
        let object_sections = if relocatable { Some(section_addresses.as_slice()) } else { None };
        elf_symbols(file, &mut i, &headers, shsize as usize, object_sections)?;
        elf_relocations(file, &mut i, &headers, shsize as usize, object_sections)?;
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
    let mut dynamic_evidence = false;
    if shoff == 0 && !relocatable {
        if let Some((doffset, dsize, dva)) = dynamic_segment {
            if dsize > 0 {
                dynamic_evidence = elf_dynamic(file, &mut i, &loads, doffset, dsize, dva)?;
            }
        }
    }
    entry(&mut i, address)?;
    if relocatable {
        push(&mut i.inventory, "ELF relocatable object sections use deterministic analysis-only addresses; symbols are rebased from section-relative values. No loader placement or relocation application is claimed.\n");
    }
    if dynamic_evidence {
        push(&mut i.inventory, "ELF static structural analysis with PT_DYNAMIC-derived dynamic-link evidence for a section-header-free image; runtime linking, Linux native attach and semantic lifting remain unavailable. PIE addresses are preferred addresses without a runtime slide.\n");
    } else {
        push(&mut i.inventory, "ELF static structural analysis with section-backed symbol and relocation tables; sectionless dynamic symbols/relocations, runtime linking, Linux native attach and semantic lifting remain unavailable. PIE addresses are preferred addresses without a runtime slide.\n");
    }
    Ok(i)
}

fn elf_symbols(file: &mut File, i: &mut Image, headers: &[u8], stride: usize, object_sections: Option<&[Option<u64>]>) -> io::Result<()> {
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
            let mut resolved_value = value;
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
                let object_base = object_sections.and_then(|sections| sections.get(section_index).copied().flatten());
                let section_offset = if object_sections.is_some() { value } else { value.saturating_sub(va) };
                // TLS values are offsets, not preferred addresses.
                if symbol_type != 6 && flags & 2 != 0
                    && (object_sections.is_some() && object_base.is_some() && value < section_size
                        || object_sections.is_none() && value >= va && value - va < section_size) {
                    if symbol_size > section_size - section_offset {
                        return Err(bad("ELF symbol extent exceeds its section"));
                    }
                    resolved_value = if let Some(base) = object_base { add(base, value)? } else { value };
                    proven_address = true;
                    if symbol_type == 2
                        && flags & 4 != 0
                        && resolved_value % i.arch.unit() == 0
                        && i.sections.iter().any(|s| s.executable && s.contains(resolved_value))
                    {
                        i.foreign
                            .functions
                            .push((resolved_value, label.clone(), source.into()));
                    }
                } else if symbol_type == 2 && flags & 2 != 0 {
                    return Err(bad("ELF function symbol lies outside its declared section"));
                }
            }
            let row = if proven_address {
                format!("0x{resolved_value:016x} name={label} source={source} type={symbol_type} binding={binding} visibility={visibility} section={section_index}{}\n", if object_sections.is_some() { " address=analysis-only-section-rebase" } else { "" })
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

fn elf_relocations(file: &mut File, i: &mut Image, headers: &[u8], stride: usize, object_sections: Option<&[Option<u64>]>) -> io::Result<()> {
    let wide = i.bits == 64;
    let be = i.be;
    let count = headers.len() / stride;
    let extent = |s: &[u8]| -> (u64, u64) {
        if wide { (u64v(&s[24..], be), u64v(&s[32..], be)) }
        else { (u32v(&s[16..], be) as u64, u32v(&s[20..], be) as u64) }
    };
    let mut records = 0usize;
    let mut bytes = 0usize;
    for s in headers.chunks_exact(stride) {
        let kind = u32v(&s[4..], be);
        if !matches!(kind, 4 | 9) { continue; }
        let (offset, size) = extent(s);
        let link = u32v(&s[if wide { 40 } else { 24 }..], be) as usize;
        let target_index = u32v(&s[if wide { 44 } else { 28 }..], be) as usize;
        let entry_size = if wide { u64v(&s[56..], be) } else { u32v(&s[36..], be) as u64 };
        let expected = match (wide, kind) { (true, 4) => 24, (true, 9) => 16, (false, 4) => 12, (false, 9) => 8, _ => unreachable!() };
        if entry_size != expected || size % expected != 0 || link == 0 || link >= count || target_index >= count
            || !matches!(u32v(&headers[link * stride + 4..], be), 2 | 11)
            || records.saturating_add((size / expected) as usize) > 200_000
            || bytes.saturating_add(size as usize) > 16 * 1024 * 1024 {
            return Err(bad("Invalid or over-budget ELF relocation table"));
        }
        let symbol_header = &headers[link * stride..];
        let (_, symbol_bytes) = extent(symbol_header);
        let symbol_stride = if wide { 24 } else { 16 };
        if symbol_bytes % symbol_stride != 0 { return Err(bad("ELF relocation links malformed symbol table")); }
        let symbol_count = symbol_bytes / symbol_stride;
        let target = &headers[target_index * stride..];
        let (target_flags, target_va, _, target_size) = if wide {
            (u64v(&target[8..], be), u64v(&target[16..], be), u64v(&target[24..], be), u64v(&target[32..], be))
        } else {
            (u32v(&target[8..], be) as u64, u32v(&target[12..], be) as u64, u32v(&target[16..], be) as u64, u32v(&target[20..], be) as u64)
        };
        range(offset, size, i.length)?;
        let table = read(file, add(i.origin, offset)?, size as usize)?;
        for record in table.chunks_exact(expected as usize) {
            let relocation_offset = if wide { u64v(record, be) } else { u32v(record, be) as u64 };
            let info = if wide { u64v(&record[8..], be) } else { u32v(&record[4..], be) as u64 };
            let symbol = if wide { info >> 32 } else { info >> 8 };
            let relocation_type = if wide { info & 0xffff_ffff } else { info & 0xff };
            if symbol >= symbol_count { return Err(bad("ELF relocation symbol index outside linked table")); }
            let object_base = object_sections.and_then(|sections| sections.get(target_index).copied().flatten());
            if object_sections.is_some() && object_base.is_none() {
                return Err(bad("ELF object relocation targets an unallocated section"));
            }
            let site = if let Some(base) = object_base {
                if relocation_offset >= target_size { return Err(bad("ELF object relocation site outside target section")); }
                add(base, relocation_offset)?
            } else {
                if target_index != 0 && target_flags & 2 != 0 && (relocation_offset < target_va || relocation_offset - target_va >= target_size) {
                    return Err(bad("ELF relocation site outside target section"));
                }
                relocation_offset
            };
            let addend = if kind == 4 {
                if wide { u64v(&record[16..], be) as i64 }
                else { u32v(&record[8..], be) as i32 as i64 }
            } else { 0 };
            push(&mut i.foreign.relocations, &format!("0x{site:016x} ELF relocation type={relocation_type} symbol-index={symbol} addend={addend} target-section={target_index} source={}\n", if kind == 4 { "SHT_RELA" } else { "SHT_REL" }));
            records += 1;
        }
        bytes += size as usize;
    }
    if records > 0 { push(&mut i.inventory, &format!("ELF relocation records={records}; references remain unapplied.\n")); }
    Ok(())
}

fn elf_dynamic(
    file: &mut File,
    i: &mut Image,
    loads: &[(u64, u64, u64, u64, u32)],
    offset: u64,
    size: u64,
    va: u64,
) -> io::Result<bool> {
    let wide = i.bits == 64;
    let be = i.be;
    let entry = if wide { 16u64 } else { 8u64 };
    if size == 0 || size % entry != 0 || size / entry > 8192 {
        return Err(bad("Invalid or over-budget ELF PT_DYNAMIC segment"));
    }
    let resolve = |want: u64, bytes: u64| -> io::Result<u64> {
        for &(a, _, off, filesz, _) in loads {
            if want >= a && bytes <= filesz && want - a <= filesz - bytes {
                return add(off, want - a);
            }
        }
        Err(bad("ELF dynamic table address outside file-backed load mappings"))
    };
    if resolve(va, size)? != offset {
        return Err(bad("ELF PT_DYNAMIC address disagrees with its load mapping"));
    }
    let bytes = read(file, add(i.origin, offset)?, size as usize)?;
    let mut entries: Vec<(i64, u64)> = Vec::new();
    for record in bytes.chunks_exact(entry as usize) {
        let tag = if wide { u64v(record, be) as i64 } else { u32v(record, be) as i32 as i64 };
        if tag == 0 {
            break;
        }
        let value = if wide { u64v(&record[8..], be) } else { u32v(&record[4..], be) as u64 };
        entries.push((tag, value));
    }
    if entries.len() as u64 == size / entry {
        return Err(bad("ELF PT_DYNAMIC is missing its DT_NULL terminator"));
    }
    let take = |wanted: i64| -> io::Result<Option<u64>> {
        let mut found = None;
        for &(tag, value) in &entries {
            if tag == wanted {
                if found.is_some() {
                    return Err(bad("Duplicate ELF dynamic tag"));
                }
                found = Some(value);
            }
        }
        Ok(found)
    };
    let rela = take(7)?;
    let relasz = take(8)?;
    let relaent = take(9)?;
    let rel = take(17)?;
    let relsz = take(18)?;
    let relent = take(19)?;
    let pltrelsz = take(2)?;
    let hash = take(4)?;
    let strtab = take(5)?;
    let symtab = take(6)?;
    let strsz = take(10)?;
    let syment = take(11)?;
    let pltrel = take(20)?;
    let jmprel = take(23)?;
    let gnu_hash = take(0x6ffffef5)?;
    if rela.is_some() != relasz.is_some() {
        return Err(bad("ELF DT_RELA relocation tags are incomplete"));
    }
    if rel.is_some() != relsz.is_some() {
        return Err(bad("ELF DT_REL relocation tags are incomplete"));
    }
    if jmprel.is_some() && (pltrelsz.is_none() || pltrel.is_none()) {
        return Err(bad("ELF DT_JMPREL relocation tags are incomplete"));
    }
    if pltrelsz.unwrap_or(0) > 0 && jmprel.is_none() {
        return Err(bad("ELF DT_PLTRELSZ without DT_JMPREL"));
    }
    let expected_rela = if wide { 24u64 } else { 12 };
    let expected_rel = if wide { 16u64 } else { 8 };
    if let Some(v) = relaent {
        if v != expected_rela {
            return Err(bad("Invalid ELF DT_RELAENT entry size"));
        }
    }
    if let Some(v) = relent {
        if v != expected_rel {
            return Err(bad("Invalid ELF DT_RELENT entry size"));
        }
    }
    if let Some(v) = pltrel {
        if jmprel.is_some() && v != 7 && v != 17 {
            return Err(bad("ELF DT_PLTREL requires DT_RELA or DT_REL"));
        }
    }
    push(&mut i.inventory, &format!("ELF PT_DYNAMIC VA={va:#x} file={offset:#x} bytes={size} entries={}\n", entries.len()));
    if [35i64, 36, 37].iter().any(|tag| entries.iter().any(|&(t, _)| t == *tag)) {
        push(&mut i.inventory, "ELF DT_RELR relative-relocation records are unsupported; references remain unapplied.\n");
    }

    let mut symbol_count: Option<u64> = None;
    let mut hash_kind = None;
    if let Some(h) = hash {
        let at = resolve(h, 8)?;
        let raw = read(file, add(i.origin, at)?, 8)?;
        let nbucket = u32v(&raw[..4], be) as u64;
        let nchain = u32v(&raw[4..], be) as u64;
        if nbucket == 0 || nbucket > 200_000 || nchain == 0 || nchain > 200_000 {
            return Err(bad("Invalid ELF DT_HASH dynamic symbol count"));
        }
        symbol_count = Some(nchain);
        hash_kind = Some("DT_HASH");
    } else if let Some(g) = gnu_hash {
        let header_at = resolve(g, 16)?;
        let header = read(file, add(i.origin, header_at)?, 16)?;
        let nbuckets = u32v(&header[..4], be) as u64;
        let symoffset = u32v(&header[4..8], be) as u64;
        let bloom_size = u32v(&header[8..12], be) as u64;
        if nbuckets == 0 || nbuckets > 200_000 || bloom_size == 0 || bloom_size > 4096 || symoffset == 0 || symoffset > 200_000 {
            return Err(bad("Invalid ELF DT_GNU_HASH header"));
        }
        let word = if wide { 8u64 } else { 4 };
        let prefix = 16 + bloom_size * word + nbuckets * 4;
        let prefix_at = resolve(g, prefix)?;
        let prefix_bytes = read(file, add(i.origin, prefix_at)?, prefix as usize)?;
        let buckets = &prefix_bytes[(16 + bloom_size * word) as usize..];
        let chains_va = g + prefix;
        let map = loads.iter().find(|&&(a, _, _, filesz, _)| g >= a && g - a < filesz);
        if map.is_none() {
            return Err(bad("ELF dynamic table address outside file-backed load mappings"));
        }
        let (map_a, map_filesz) = (map.unwrap().0, map.unwrap().3);
        let chains_available = map_filesz - (chains_va - map_a);
        let chains_bytes = chains_available.min(800_000);
        let chains_at = resolve(chains_va, chains_bytes)?;
        let chains = read(file, add(i.origin, chains_at)?, chains_bytes as usize)?;
        let mut count = symoffset;
        for slot in 0..nbuckets {
            let start = u32v(&buckets[slot as usize * 4..], be) as u64;
            if start == 0 {
                continue;
            }
            if start < symoffset {
                return Err(bad("ELF DT_GNU_HASH bucket below its symbol offset"));
            }
            let mut index = start;
            loop {
                let chain_slot = index - symoffset;
                let at = chain_slot * 4;
                if at + 4 > chains.len() as u64 {
                    return Err(bad("ELF DT_GNU_HASH chain outside its bounded window"));
                }
                let chain = u32v(&chains[at as usize..], be);
                index += 1;
                if index > 200_000 {
                    return Err(bad("ELF DT_GNU_HASH dynamic symbol budget exceeded"));
                }
                if chain & 1 != 0 {
                    break;
                }
            }
            if index > count {
                count = index;
            }
        }
        if count == 0 || count > 200_000 {
            return Err(bad("Invalid ELF DT_GNU_HASH dynamic symbol count"));
        }
        symbol_count = Some(count);
        hash_kind = Some("DT_GNU_HASH");
    }

    let mut recovered_symbols = false;
    if let Some(nchain) = symbol_count {
        let symtab_va = symtab.ok_or_else(|| bad("ELF DT_HASH/DT_GNU_HASH without DT_SYMTAB"))?;
        let strtab_va = strtab.ok_or_else(|| bad("ELF dynamic symbols without DT_STRTAB"))?;
        let strsz = strsz.ok_or_else(|| bad("ELF dynamic symbols without DT_STRSZ"))?;
        let syment = syment.ok_or_else(|| bad("ELF dynamic symbols without DT_SYMENT"))?;
        if syment != if wide { 24u64 } else { 16 } {
            return Err(bad("Invalid ELF DT_SYMENT entry size"));
        }
        let symtab_bytes = nchain * syment;
        if symtab_bytes > 16 * 1024 * 1024 || strsz > 8 * 1024 * 1024 {
            return Err(bad("ELF dynamic symbol/string byte budget exceeded"));
        }
        if i.foreign.symbol_count.saturating_add(nchain as usize) > 200_000 {
            return Err(bad("ELF dynamic symbol record budget exceeded"));
        }
        let sym_at = resolve(symtab_va, symtab_bytes)?;
        let str_at = resolve(strtab_va, strsz)?;
        let strings = read(file, add(i.origin, str_at)?, strsz as usize)?;
        if strings.first() != Some(&0) || strings.last() != Some(&0) {
            return Err(bad("Invalid ELF dynamic string-table framing"));
        }
        let table = read(file, add(i.origin, sym_at)?, symtab_bytes as usize)?;
        let source = "ELF dynamic-symbol";
        let mut names_budget = 1024 * 1024usize;
        for record in table.chunks_exact(syment as usize) {
            i.foreign.symbol_count += 1;
            let index = u32v(record, be) as usize;
            let (value, symbol_size, info, visibility, section_index) = if wide {
                (u64v(&record[8..], be), u64v(&record[16..], be), record[4], record[5] & 3, half(&record[6..], be) as usize)
            } else {
                (u32v(&record[4..], be) as u64, u32v(&record[8..], be) as u64, record[12], record[13] & 3, half(&record[14..], be) as usize)
            };
            if section_index == 0xffff {
                return Err(bad("Extended ELF symbol section indexes are unsupported"));
            }
            if index >= strings.len() {
                return Err(bad("ELF symbol name outside string table"));
            }
            if index == 0 {
                continue;
            }
            let tail = &strings[index..strings.len().min(index + 1025)];
            let end = tail.iter().position(|&b| b == 0).ok_or_else(|| bad("Unterminated or over-budget ELF symbol name"))?;
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
            if symbol_type != 6 && section_index < 0xff00 {
                if let Some(&(a, m, _, _, _)) = loads.iter().find(|&&(a, m, _, _, _)| value >= a && value - a < m) {
                    if symbol_size > 0 && value - a + symbol_size > m {
                        return Err(bad("ELF dynamic symbol extent exceeds its load mapping"));
                    }
                    proven_address = true;
                    if symbol_type == 2
                        && value % i.arch.unit() == 0
                        && i.sections.iter().any(|s| s.executable && s.contains(value))
                    {
                        i.foreign.functions.push((value, label.clone(), source.into()));
                    }
                }
            }
            let row = if proven_address {
                format!("0x{value:016x} name={label} source={source} type={symbol_type} binding={binding} visibility={visibility} section={section_index}\n")
            } else {
                format!("name={label} value={value:#x} source={source} type={symbol_type} binding={binding} section={section_index}; address unresolved\n")
            };
            push(&mut i.foreign.symbols, &row);
            if matches!(binding, 1 | 2) && matches!(visibility, 0 | 3) {
                push(&mut i.foreign.exports, &row);
            }
        }
        recovered_symbols = true;
        let kind = hash_kind.unwrap_or("DT_HASH");
        push(&mut i.inventory, &format!("ELF dynamic-symbol records={nchain} (including unnamed records); undefined names remain address-free; {kind} proven symbol count={nchain}.\n"));
    } else {
        push(&mut i.inventory, "DT_HASH and DT_GNU_HASH are absent; dynamic symbol names/counts remain unrecovered and relocation symbol indices are transcribed without a proven dynamic symbol count.\n");
    }

    let mut records = 0usize;
    let mut table_bytes = 0usize;
    let mut tables: Vec<(u64, u64, bool, &'static str)> = Vec::new();
    if let Some(at) = rela {
        let sz = relasz.unwrap_or(0);
        if sz > 0 {
            tables.push((at, sz, true, "DT_RELA"));
        }
    }
    if let Some(at) = rel {
        let sz = relsz.unwrap_or(0);
        if sz > 0 {
            tables.push((at, sz, false, "DT_REL"));
        }
    }
    if let Some(at) = jmprel {
        let sz = pltrelsz.unwrap_or(0);
        if sz > 0 {
            tables.push((at, sz, pltrel == Some(7), "DT_JMPREL"));
        }
    }
    for (want, sz, explicit, source) in tables {
        let entsize = if explicit { expected_rela } else { expected_rel };
        if sz % entsize != 0 {
            return Err(bad("Invalid ELF dynamic relocation table extent"));
        }
        let count = sz / entsize;
        if records.saturating_add(count as usize) > 200_000
            || table_bytes.saturating_add(sz as usize) > 16 * 1024 * 1024
        {
            return Err(bad("ELF dynamic relocation record or byte budget exceeded"));
        }
        let at = resolve(want, sz)?;
        let raw = read(file, add(i.origin, at)?, sz as usize)?;
        for record in raw.chunks_exact(entsize as usize) {
            let site = if wide { u64v(record, be) } else { u32v(record, be) as u64 };
            let info = if wide { u64v(&record[8..], be) } else { u32v(&record[4..], be) as u64 };
            let symbol = if wide { info >> 32 } else { info >> 8 };
            let relocation_type = if wide { info & 0xffff_ffff } else { info & 0xff };
            if !loads.iter().any(|&(a, m, _, _, _)| site >= a && site - a < m) {
                return Err(bad("ELF dynamic relocation site outside load mappings"));
            }
            if let Some(nchain) = symbol_count {
                if symbol >= nchain {
                    return Err(bad("ELF dynamic relocation symbol index outside dynamic symbol count"));
                }
            }
            let addend = if explicit {
                if wide { u64v(&record[16..], be) as i64 } else { u32v(&record[8..], be) as i32 as i64 }
            } else {
                0
            };
            push(&mut i.foreign.relocations, &format!("0x{site:016x} ELF relocation type={relocation_type} symbol-index={symbol} addend={addend} source={source}\n"));
            records += 1;
        }
        table_bytes += sz as usize;
    }
    if records > 0 {
        push(&mut i.inventory, &format!("ELF dynamic relocation records={records}; references remain unapplied.\n"));
    }
    Ok(records > 0 || recovered_symbols)
}

fn resolve_rva(extents: &[(u64, u64, u64, u64)], rva: u64, bytes: u64) -> io::Result<u64> {
    for &(s_rva, mem_size, file_off, raw_size) in extents {
        let back = if raw_size > 0 { raw_size.min(mem_size) } else { mem_size };
        if rva >= s_rva && bytes <= back && rva - s_rva <= back - bytes {
            return add(file_off, rva - s_rva);
        }
    }
    Err(bad("PE data outside section mappings"))
}

fn read_cstr(file: &mut File, i: &Image, extents: &[(u64, u64, u64, u64)], rva: u64, max: usize) -> io::Result<String> {
    let mut avail = 0u64;
    for &(s_rva, mem_size, _, raw_size) in extents {
        if rva >= s_rva && rva < s_rva + mem_size {
            let back = if raw_size > 0 { raw_size.min(mem_size) } else { mem_size };
            avail = (s_rva + back) - rva;
            break;
        }
    }
    if avail == 0 {
        return Err(bad("PE data outside section mappings"));
    }
    let n = avail.min(max as u64) as usize;
    let rel = resolve_rva(extents, rva, n as u64)?;
    let buf = read(file, add(i.origin, rel)?, n)?;
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    Ok(String::from_utf8_lossy(&buf[..end]).to_string())
}

fn pe_data_directories(
    file: &mut File,
    i: &mut Image,
    base: u64,
    extents: &[(u64, u64, u64, u64)],
    p: &[u8],
    directory_offset: u64,
    directories: u64,
    bits: u32,
) -> io::Result<bool> {
    let ptr = if bits == 64 { 8u64 } else { 4 };
    let dir = |index: usize| -> (u64, u64) {
        if (index as u64) >= directories {
            return (0, 0);
        }
        let off = (directory_offset + (index as u64) * 8) as usize;
        if off + 8 > p.len() {
            return (0, 0);
        }
        (u32v(&p[off..], false) as u64, u32v(&p[off + 4..], false) as u64)
    };
    let mut any = false;

    // Imports (data directory 1).
    let (imp_rva, _) = dir(1);
    if imp_rva != 0 {
        let mut desc_rva = imp_rva;
        let mut desc_count = 0usize;
        loop {
            let rel = resolve_rva(extents, desc_rva, 20)?;
            let desc = read(file, add(i.origin, rel)?, 20)?;
            let oft = u32v(&desc[0..], false) as u64;
            let ts = u32v(&desc[4..], false);
            let fc = u32v(&desc[8..], false);
            let name_rva = u32v(&desc[12..], false) as u64;
            let ft = u32v(&desc[16..], false) as u64;
            if oft == 0 && name_rva == 0 && ft == 0 && ts == 0 && fc == 0 {
                break;
            }
            desc_count += 1;
            if desc_count > 4096 {
                return Err(bad("PE import descriptor budget exceeded"));
            }
            if name_rva != 0 {
                let dll = read_cstr(file, i, extents, name_rva, 260)?;
                if !dll.is_empty() {
                    let thunk_rva = if oft != 0 { oft } else { ft };
                    if thunk_rva != 0 {
                        let mut t_rva = thunk_rva;
                        let mut t_count = 0usize;
                        loop {
                            let t_rel = resolve_rva(extents, t_rva, ptr)?;
                            let raw = read(file, add(i.origin, t_rel)?, ptr as usize)?;
                            let thunk = if bits == 64 { u64v(&raw, false) } else { u32v(&raw, false) as u64 };
                            if thunk == 0 {
                                break;
                            }
                            t_count += 1;
                            if t_count > 100_000 {
                                return Err(bad("PE import thunk budget exceeded"));
                            }
                            let high = if bits == 64 { 1u64 << 63 } else { 1u64 << 31 };
                            if thunk & high != 0 {
                                let ordinal = (thunk & 0xffff) as u16;
                                push(&mut i.foreign.imports, &format!("dll={dll} ordinal={ordinal} source=PE import\n"));
                            } else {
                                let hn_rva = thunk & !high;
                                if hn_rva != 0 {
                                    let hn_rel = resolve_rva(extents, hn_rva, 2)?;
                                    let hint_buf = read(file, add(i.origin, hn_rel)?, 2)?;
                                    let hint = u16::from_le_bytes([hint_buf[0], hint_buf[1]]);
                                    let name = read_cstr(file, i, extents, hn_rva + 2, 258)?;
                                    if !name.is_empty() {
                                        push(&mut i.foreign.imports, &format!("dll={dll} name={name} hint={hint} source=PE import\n"));
                                    }
                                }
                            }
                            t_rva += ptr;
                        }
                    }
                    any = true;
                }
            }
            desc_rva += 20;
        }
        push(&mut i.inventory, &format!("PE import descriptors={desc_count}.\n"));
    }

    // Exports (data directory 0).
    let (exp_rva, _) = dir(0);
    if exp_rva != 0 {
        let rel = resolve_rva(extents, exp_rva, 40)?;
        let dir_bytes = read(file, add(i.origin, rel)?, 40)?;
        let name_rva = u32v(&dir_bytes[12..], false) as u64;
        let num_funcs = u32v(&dir_bytes[20..], false) as u64;
        let num_names = u32v(&dir_bytes[24..], false) as u64;
        let aof_rva = u32v(&dir_bytes[28..], false) as u64;
        let aon_rva = u32v(&dir_bytes[32..], false) as u64;
        let aono_rva = u32v(&dir_bytes[36..], false) as u64;
        if num_funcs > 100_000 || num_names > 100_000 {
            return Err(bad("PE export budget exceeded"));
        }
        let mut func_rvas: Vec<u64> = Vec::new();
        if aof_rva != 0 && num_funcs > 0 {
            let aof_rel = resolve_rva(extents, aof_rva, num_funcs * 4)?;
            let aof_bytes = read(file, add(i.origin, aof_rel)?, (num_funcs * 4) as usize)?;
            for k in 0..num_funcs as usize {
                func_rvas.push(u32v(&aof_bytes[k * 4..], false) as u64);
            }
        }
        let mut name_rvas: Vec<u64> = Vec::new();
        if aon_rva != 0 && num_names > 0 {
            let aon_rel = resolve_rva(extents, aon_rva, num_names * 4)?;
            let aon_bytes = read(file, add(i.origin, aon_rel)?, (num_names * 4) as usize)?;
            for k in 0..num_names as usize {
                name_rvas.push(u32v(&aon_bytes[k * 4..], false) as u64);
            }
        }
        let mut ordinals: Vec<u16> = Vec::new();
        if aono_rva != 0 && num_names > 0 {
            let aono_rel = resolve_rva(extents, aono_rva, num_names * 2)?;
            let aono_bytes = read(file, add(i.origin, aono_rel)?, (num_names * 2) as usize)?;
            for k in 0..num_names as usize {
                ordinals.push(u16::from_le_bytes([aono_bytes[k * 2], aono_bytes[k * 2 + 1]]));
            }
        }
        for k in 0..name_rvas.len() {
            let ename = read_cstr(file, i, extents, name_rvas[k], 260)?;
            if ename.is_empty() {
                continue;
            }
            let func_index = if k < ordinals.len() { ordinals[k] as usize } else { k };
            if func_index >= func_rvas.len() {
                continue;
            }
            let func_rva = func_rvas[func_index];
            if func_rva == 0 {
                continue;
            }
            let va = base + func_rva;
            push(&mut i.foreign.exports, &format!("0x{va:016x} name={ename} source=PE export\n"));
            any = true;
        }
        if name_rva != 0 {
            let dll = read_cstr(file, i, extents, name_rva, 260)?;
            if !dll.is_empty() {
                push(&mut i.inventory, &format!("PE export module={dll}\n"));
            }
        }
        push(&mut i.inventory, &format!("PE export names={num_names} functions={num_funcs}.\n"));
    }

    // Base relocations (data directory 5).
    let (br_rva, br_size) = dir(5);
    if br_rva != 0 && br_size > 0 {
        let mut off = 0u64;
        let mut reloc_count = 0usize;
        while off + 8 <= br_size {
            let block_rva = br_rva + off;
            let rel = resolve_rva(extents, block_rva, 8)?;
            let header = read(file, add(i.origin, rel)?, 8)?;
            let page_rva = u32v(&header[0..], false) as u64;
            let block_size = u32v(&header[4..], false) as u64;
            if block_size < 8 || block_size % 2 != 0 || off + block_size > br_size {
                return Err(bad("Invalid PE base relocation block"));
            }
            let num_entries = ((block_size - 8) / 2) as usize;
            if num_entries > 100_000 {
                return Err(bad("PE base relocation budget exceeded"));
            }
            if num_entries > 0 {
                let entries_rel = resolve_rva(extents, block_rva + 8, (num_entries * 2) as u64)?;
                let entries_bytes = read(file, add(i.origin, entries_rel)?, num_entries * 2)?;
                for k in 0..num_entries {
                    let entry = u16::from_le_bytes([entries_bytes[k * 2], entries_bytes[k * 2 + 1]]);
                    let rtype = (entry >> 12) as u64;
                    let offset = (entry & 0xfff) as u64;
                    if rtype == 0 {
                        continue;
                    }
                    let site_rva = page_rva + offset;
                    let site_va = base + site_rva;
                    push(&mut i.foreign.relocations, &format!("0x{site_va:016x} PE base relocation type={rtype} source=PE base relocation\n"));
                    reloc_count += 1;
                }
            }
            off += block_size;
            if block_size == 0 {
                break;
            }
        }
        if reloc_count > 0 {
            any = true;
        }
        push(&mut i.inventory, &format!("PE base relocation records={reloc_count}.\n"));
    }

    if any {
        push(&mut i.inventory, "PE data-directory evidence recovered (imports/exports/base relocations).\n");
    }
    Ok(any)
}

pub(super) fn coff(file: &mut File, o: &Options) -> io::Result<Image> {
    let length = file.metadata()?.len() - o.offset;
    range(0, 20, length)?;
    let h = read(file, o.offset, 20)?;
    let machine = half(&h[0..], false);
    let num_sections = half(&h[2..], false) as u64;
    let time_date = u32v(&h[4..], false);
    let ptr_symtab = u32v(&h[8..], false) as u64;
    let num_symbols = u32v(&h[12..], false) as u64;
    let opt_size = half(&h[16..], false) as u64;
    let characteristics = half(&h[18..], false) as u32;
    let arch = match machine {
        0x014c => Arch::X86,
        0x8664 => Arch::X64,
        0xaa64 => Arch::A64,
        _ => return Err(bad("Unsupported COFF machine")),
    };
    let bits = if machine == 0x014c { 32u32 } else { 64 };
    if num_sections == 0 || num_sections > 96 {
        return Err(bad("Invalid COFF section count"));
    }
    if opt_size != 0 {
        return Err(bad("COFF objects must not carry an optional header"));
    }
    if num_symbols > 200_000 {
        return Err(bad("COFF symbol count over budget"));
    }
    let mut i = image("COFF", arch, bits, false, o, length)?;
    i.cpu = machine as u32;
    i.flags = characteristics;
    // Section table.
    let sec_table_offset = 20u64;
    let sec_table_bytes = num_sections * 40;
    range(sec_table_offset, sec_table_bytes, length)?;
    let sec_table = read(file, add(o.offset, sec_table_offset)?, sec_table_bytes as usize)?;
    // Symbol table + string table.
    let mut string_table: Vec<u8> = Vec::new();
    if num_symbols > 0 {
        range(ptr_symtab, num_symbols * 18, length)?;
        let st_off = add(ptr_symtab, num_symbols * 18)?;
        range(st_off, 4, length)?;
        let sz_bytes = read(file, add(o.offset, st_off)?, 4)?;
        let st_size = u32v(&sz_bytes, false) as u64;
        if st_size < 4 || st_size > 16 * 1024 * 1024 {
            return Err(bad("Invalid COFF string table size"));
        }
        range(st_off, st_size, length)?;
        string_table = read(file, add(o.offset, st_off)?, st_size as usize)?;
    }
    // Analysis-only section bases (4KB stride).
    let mut section_base: Vec<u64> = Vec::with_capacity(num_sections as usize);
    let mut base = 0x10000u64;
    for _ in 0..num_sections {
        section_base.push(base);
        base = base.saturating_add(0x1000);
    }
    // Section names helper.
    let section_name = |name_bytes: &[u8]| -> String {
        if name_bytes[0] == b'/' {
            // /N string-table reference
            let idx_str = String::from_utf8_lossy(&name_bytes[1..]).trim_end_matches('\0').to_string();
            if let Ok(idx) = idx_str.parse::<usize>() {
                if idx > 0 && idx + 1 < string_table.len() {
                    let tail = &string_table[idx..];
                    let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
                    return String::from_utf8_lossy(&tail[..end]).to_string();
                }
            }
            format!("/{}", idx_str)
        } else {
            let end = name_bytes.iter().position(|&b| b == 0).unwrap_or(name_bytes.len());
            String::from_utf8_lossy(&name_bytes[..end]).to_string()
        }
    };
    // Parse sections.
    let mut section_raw: Vec<(u64, u64, u64, u64, u32)> = Vec::new(); // (rva-ish base, raw_size, file_off, mem, chars)
    for (idx, s) in sec_table.chunks_exact(40).enumerate() {
        let name = section_name(&s[..8]);
        let virtual_size = u32v(&s[8..], false) as u64;
        let virtual_address = u32v(&s[12..], false) as u64;
        let raw_size = u32v(&s[16..], false) as u64;
        let raw_offset = u32v(&s[20..], false) as u64;
        let ptr_reloc = u32v(&s[24..], false) as u64;
        let _num_reloc = u32v(&s[32..], false) as u64; // u16 at 32? Actually NumberOfRelocations is u16 at offset 32, NumberOfLinenumbers u16 at 34
        let num_reloc = u16::from_le_bytes([s[32], s[33]]) as u64;
        let characteristics = u32v(&s[36..], false);
        if raw_size > 0 {
            range(raw_offset, raw_size, length)?;
        }
        let sb = section_base[idx];
        if raw_size > 0 {
            section(
                &mut i,
                Section {
                    name: name.clone(),
                    segment: "COFF".into(),
                    address: sb,
                    size: raw_size,
                    offset: add(o.offset, raw_offset)?,
                    executable: characteristics & 0x20000000 != 0,
                    ..Section::default()
                },
            )?;
        }
        section_raw.push((sb, raw_size, raw_offset, virtual_size.max(raw_size), characteristics));
        // Relocations for this section.
        if num_reloc > 0 && ptr_reloc > 0 {
            let reloc_bytes = num_reloc * 10;
            range(ptr_reloc, reloc_bytes, length)?;
            let relocs = read(file, add(o.offset, ptr_reloc)?, reloc_bytes as usize)?;
            for r in relocs.chunks_exact(10) {
                let va = u32v(&r[0..], false) as u64;
                let sym_idx = u32v(&r[4..], false) as u64;
                let rtype = u16::from_le_bytes([r[8], r[9]]) as u64;
                let site = sb + va;
                push(&mut i.foreign.relocations, &format!("0x{site:016x} COFF relocation type={rtype} symbol-index={sym_idx} source=COFF relocation\n"));
            }
            push(&mut i.inventory, &format!("COFF section {name} relocations={num_reloc}.\n"));
        }
        let _ = virtual_address;
    }
    // Symbols.
    if num_symbols > 0 {
        let symtab = read(file, add(o.offset, ptr_symtab)?, (num_symbols * 18) as usize)?;
        let mut idx = 0u64;
        while idx < num_symbols {
            let rec = &symtab[(idx as usize) * 18..(idx as usize) * 18 + 18];
            let name_bytes = &rec[0..8];
            let value = u32v(&rec[8..], false) as u64;
            let section_number = i16::from_le_bytes([rec[12], rec[13]]);
            let sym_type = u16::from_le_bytes([rec[14], rec[15]]);
            let storage_class = rec[16];
            let num_aux = rec[17] as u64;
            i.foreign.symbol_count += 1;
            // Name resolution.
            let name = if name_bytes[..4] == [0, 0, 0, 0] {
                let off = u32v(&name_bytes[4..], false) as usize;
                if off > 0 && off < string_table.len() {
                    let tail = &string_table[off..];
                    let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
                    String::from_utf8_lossy(&tail[..end]).to_string()
                } else {
                    String::new()
                }
            } else {
                let end = name_bytes.iter().position(|&b| b == 0).unwrap_or(8);
                String::from_utf8_lossy(&name_bytes[..end]).to_string()
            };
            let section_idx = if section_number > 0 { (section_number - 1) as usize } else { usize::MAX };
            if !name.is_empty() {
                if section_number == 0 {
                    // Undefined external.
                    push(&mut i.foreign.imports, &format!("name={name} storage-class={storage_class} source=COFF symbol\n"));
                } else if section_number > 0 && section_idx < section_raw.len() {
                    let sb = section_base[section_idx];
                    let va = sb + value;
                    let row = format!("0x{va:016x} name={name} source=COFF symbol type={sym_type} storage-class={storage_class} section={section_number}\n");
                    push(&mut i.foreign.symbols, &row);
                    // Function detection: storage class 2 (external) or 3 (static) with type 0x20 in code section.
                    let is_code = section_raw[section_idx].4 & 0x00000020 != 0;
                    if sym_type == 0x20 && is_code {
                        i.foreign.functions.push((va, name.clone(), "COFF symbol".into()));
                    }
                    if storage_class == 2 {
                        push(&mut i.foreign.exports, &row);
                    }
                }
            }
            idx += 1 + num_aux;
        }
    }
    // Entry point: none for objects.
    let _ = time_date;
    push(&mut i.inventory, &format!("COFF machine={machine:#x}; sections={}; symbols={}; string-table={} bytes.\nCOFF object structural analysis; relocations remain unapplied evidence.\n", num_sections, num_symbols, string_table.len()));
    Ok(i)
}

pub(super) fn pdb(file: &mut File, o: &Options) -> io::Result<Image> {
    const MSF_SIG: &[u8] = b"Microsoft C/C++ MSF 7.00\r\n\x1aDS\0\0\0";
    let length = file.metadata()?.len() - o.offset;
    if length < 56 {
        return Err(bad("PDB file too small for MSF superblock"));
    }
    let sig = read(file, o.offset, 32)?;
    if sig != MSF_SIG {
        return Err(bad("Invalid PDB MSF 7.0 signature"));
    }
    let sb = read(file, add(o.offset, 32)?, 24)?;
    let block_size = u32v(&sb[0..4], false) as u64;
    let free_block_map = u32v(&sb[4..8], false) as u64;
    let num_blocks = u32v(&sb[8..12], false) as u64;
    let num_directory_bytes = u32v(&sb[12..16], false) as u64;
    let _unknown = u32v(&sb[16..20], false);
    let block_map_addr = u32v(&sb[20..24], false) as u64;
    // Validate block size.
    if !matches!(block_size, 512 | 1024 | 4096) {
        return Err(bad("Invalid PDB block size"));
    }
    if num_blocks == 0 || num_blocks > 1 << 24 {
        return Err(bad("Invalid PDB block count"));
    }
    if num_blocks * block_size > length {
        return Err(bad("PDB block count exceeds file size"));
    }
    if num_blocks * block_size != length {
        return Err(bad("PDB file size is not a multiple of block size"));
    }
    if free_block_map > 2 {
        return Err(bad("Invalid PDB free block map index"));
    }
    if block_map_addr >= num_blocks {
        return Err(bad("PDB block map address out of range"));
    }
    if num_directory_bytes == 0 || num_directory_bytes > 256 * 1024 * 1024 {
        return Err(bad("Invalid PDB directory size"));
    }
    // Read block map: array of u32 block indices, enough to cover num_directory_bytes.
    let dir_blocks_needed = (num_directory_bytes + block_size - 1) / block_size;
    if dir_blocks_needed > num_blocks {
        return Err(bad("PDB directory block count exceeds total blocks"));
    }
    let block_map_offset = add(o.offset, block_map_addr * block_size)?;
    let block_map_size = dir_blocks_needed * 4;
    if block_map_size > block_size {
        // Block map may span multiple blocks, but typically fits in one.
        // Read from the block map address for dir_blocks_needed entries.
    }
    range(block_map_addr * block_size, dir_blocks_needed * 4, length)?;
    // Read directory block indices.
    let mut dir_block_indices: Vec<u64> = Vec::with_capacity(dir_blocks_needed as usize);
    // The block map is at block_map_addr, containing dir_blocks_needed u32 entries.
    // But the block map itself may be larger. Read dir_blocks_needed * 4 bytes.
    let bm_data = read(file, block_map_offset, (dir_blocks_needed * 4) as usize)?;
    for chunk in bm_data.chunks_exact(4) {
        let idx = u32v(chunk, false) as u64;
        if idx >= num_blocks {
            return Err(bad("PDB directory block index out of range"));
        }
        dir_block_indices.push(idx);
    }
    // Assemble directory bytes.
    let mut directory: Vec<u8> = Vec::with_capacity(num_directory_bytes as usize);
    for &blk in &dir_block_indices {
        let blk_off = add(o.offset, blk * block_size)?;
        let to_read = block_size.min(num_directory_bytes as u64 - directory.len() as u64) as usize;
        let data = read(file, blk_off, to_read)?;
        directory.extend_from_slice(&data);
        if directory.len() as u64 >= num_directory_bytes {
            break;
        }
    }
    if (directory.len() as u64) < num_directory_bytes {
        return Err(bad("PDB directory truncated"));
    }
    directory.truncate(num_directory_bytes as usize);
    // Parse directory header.
    if directory.len() < 4 {
        return Err(bad("PDB directory too small"));
    }
    let num_streams = u32v(&directory[0..4], false) as u64;
    if num_streams > 100_000 {
        return Err(bad("PDB stream count over budget"));
    }
    let mut off = 4usize;
    // Read stream sizes.
    let mut stream_sizes: Vec<u64> = Vec::with_capacity(num_streams as usize);
    for _ in 0..num_streams {
        if off + 4 > directory.len() {
            return Err(bad("PDB directory truncated reading stream sizes"));
        }
        let sz = u32v(&directory[off..off + 4], false) as u64;
        stream_sizes.push(sz);
        off += 4;
    }
    // Read block indices for each stream.
    let mut stream_blocks: Vec<Vec<u64>> = Vec::with_capacity(num_streams as usize);
    for &sz in &stream_sizes {
        let n = if sz == 0 { 0 } else { (sz + block_size - 1) / block_size };
        if n > num_blocks {
            return Err(bad("PDB stream block count exceeds total blocks"));
        }
        let mut blocks = Vec::with_capacity(n as usize);
        for _ in 0..n {
            if off + 4 > directory.len() {
                return Err(bad("PDB directory truncated reading stream blocks"));
            }
            let idx = u32v(&directory[off..off + 4], false) as u64;
            if idx >= num_blocks && sz > 0 {
                return Err(bad("PDB stream block index out of range"));
            }
            blocks.push(idx);
            off += 4;
        }
        stream_blocks.push(blocks);
    }
    // Build Image.
    let mut i = image("PDB", Arch::X86, 32, false, o, length)?;
    i.cpu = 0;
    // Inventory.
    push(&mut i.inventory, &format!("PDB MSF 7.0 block_size={} num_blocks={} directory_bytes={} streams={}\n", block_size, num_blocks, num_directory_bytes, num_streams));
    for (idx, (sz, blocks)) in stream_sizes.iter().zip(stream_blocks.iter()).enumerate() {
        push(&mut i.inventory, &format!("PDB stream {} size={} blocks={}\n", idx, sz, blocks.len()));
    }
    // Identify well-known streams.
    let mut foreign_recovered = false;
    // Stream 1 = PDB Info (if present).
    if num_streams > 1 && stream_sizes[1] >= 4 {
        let (sz, blocks) = (&stream_sizes[1], &stream_blocks[1]);
        let mut stream_data: Vec<u8> = Vec::with_capacity(*sz as usize);
        for &blk in blocks {
            let blk_off = add(o.offset, blk * block_size)?;
            let to_read = block_size.min(*sz - stream_data.len() as u64) as usize;
            let data = read(file, blk_off, to_read)?;
            stream_data.extend_from_slice(&data);
            if stream_data.len() as u64 >= *sz {
                break;
            }
        }
        stream_data.truncate(*sz as usize);
        if stream_data.len() >= 4 {
            let version = u32v(&stream_data[0..4], false);
            let version_name = match version {
                19990903 => "VC70 (2003)",
                20000404 => "VC80 (2005)",
                20091201 => "VC110 (2012)",
                20140508 => "VC140 (2015)",
                _ => "unknown",
            };
            push(&mut i.inventory, &format!("PDB info stream version={} ({})\n", version, version_name));
            if stream_data.len() >= 24 {
                let signature = u32v(&stream_data[4..8], false);
                let age = u32v(&stream_data[16..20], false);
                push(&mut i.inventory, &format!("PDB info signature={} age={}\n", signature, age));
            }
            foreign_recovered = true;
        }
    }
    // Stream 2 = TPI.
    if num_streams > 2 && stream_sizes[2] > 0 {
        push(&mut i.inventory, &format!("PDB TPI stream size={}\n", stream_sizes[2]));
        foreign_recovered = true;
    }
    // Stream 3 = DBI.
    if num_streams > 3 && stream_sizes[3] > 0 {
        push(&mut i.inventory, &format!("PDB DBI stream size={}\n", stream_sizes[3]));
        foreign_recovered = true;
    }
    if foreign_recovered {
        push(&mut i.inventory, "PDB MSF container validated with stream directory and known streams.\n");
    } else {
        push(&mut i.inventory, "PDB MSF container validated; no recognized streams found.\n");
    }
    let _ = (free_block_map, _unknown);
    Ok(i)
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
    let pe_recovered = pe_data_directories(file, &mut i, base, &extents, &p, directory_offset, directories, bits)?;
    if pe_recovered {
        push(&mut i.inventory, &format!("PE preferred image base={base:#x}; image extent={image_size:#x}; data directories={directories}.\nPE static structural analysis with recovered imports, exports and base relocations; PDB, Authenticode validation, Windows execution and semantic lifting remain unavailable. Standalone COFF objects are unsupported.\n"));
    } else {
        push(&mut i.inventory, &format!("PE preferred image base={base:#x}; image extent={image_size:#x}; data directories={directories}.\nPE static structural analysis; imports/exports and base relocations recovered; PDB, Authenticode validation, Windows execution and semantic lifting remain unavailable.\n"));
    }
    Ok(i)
}
