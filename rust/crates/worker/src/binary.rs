//! Bounded, read-only multi-architecture import and structural analysis.
//! Metadata facts and heuristic candidates are deliberately kept distinct.
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{CStr, CString},
    fs::{File, Metadata},
    io::{self, Read, Seek, SeekFrom},
    os::{
        fd::FromRawFd,
        raw::{c_char, c_void},
        unix::fs::MetadataExt,
    },
};
const SCAN: usize = 4 * 1024 * 1024;
const TEXT: usize = 60_000;
fn bad(s: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, s)
}
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub format: String,
    pub architecture: String,
    pub base: u64,
    pub offset: u64,
}
impl Options {
    pub fn parse(args: &[String]) -> io::Result<Self> {
        if args.len() != 4 {
            return Err(bad(
                "Import requires format, architecture, load address and image offset",
            ));
        }
        let number = |s: &str| -> io::Result<u64> {
            if let Some(h) = s.strip_prefix("0x") {
                u64::from_str_radix(h, 16).map_err(|_| bad("Invalid hexadecimal mapping"))
            } else {
                s.parse().map_err(|_| bad("Invalid mapping number"))
            }
        };
        let o = Self {
            format: args[0].clone(),
            architecture: args[1].clone(),
            base: number(&args[2])?,
            offset: number(&args[3])?,
        };
        if !["auto", "macho", "sepos", "securerom", "iboot", "raw"].contains(&o.format.as_str())
            || !["auto", "aarch64", "i386", "x86_64", "arm", "thumb"]
                .contains(&o.architecture.as_str())
        {
            return Err(bad("Unsupported import configuration"));
        }
        Ok(o)
    }
    pub fn json(&self) -> String {
        format!(
            "{{\"format\":{},\"architecture\":{},\"load_address\":{},\"file_offset\":{}}}",
            super::quote(&self.format),
            super::quote(&self.architecture),
            super::quote(&self.base.to_string()),
            super::quote(&self.offset.to_string())
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Arch {
    A64 = 1,
    X86 = 2,
    X64 = 3,
    Arm = 4,
    Thumb = 5,
}
impl Arch {
    fn name(self) -> &'static str {
        match self {
            Self::A64 => "AArch64",
            Self::X86 => "Intel i386",
            Self::X64 => "Intel x86_64",
            Self::Arm => "ARM32",
            Self::Thumb => "ARM32 Thumb-2",
        }
    }
    fn unit(self) -> u64 {
        match self {
            Self::A64 | Self::Arm => 4,
            Self::Thumb => 2,
            _ => 1,
        }
    }
    fn width(self) -> usize {
        if matches!(self, Self::A64 | Self::X64) {
            8
        } else {
            4
        }
    }
    fn option(s: &str) -> Option<Self> {
        match s {
            "aarch64" => Some(Self::A64),
            "i386" => Some(Self::X86),
            "x86_64" => Some(Self::X64),
            "arm" => Some(Self::Arm),
            "thumb" => Some(Self::Thumb),
            _ => None,
        }
    }
    fn cpu(cpu: u32) -> io::Result<Self> {
        match cpu {
            0x0100000c => Ok(Self::A64),
            7 => Ok(Self::X86),
            0x01000007 => Ok(Self::X64),
            12 => Ok(Self::Thumb),
            _ => Err(bad("Mach-O CPU is unsupported")),
        }
    }
    fn compatible(self, cpu: u32) -> bool {
        matches!(
            (self, cpu),
            (Self::A64, 0x0100000c)
                | (Self::X86, 7)
                | (Self::X64, 0x01000007)
                | (Self::Arm | Self::Thumb, 12)
        )
    }
}
#[derive(Clone, Debug, Default)]
struct Section {
    name: String,
    segment: String,
    address: u64,
    size: u64,
    offset: u64,
    flags: u32,
    reloc: u64,
    nreloc: u32,
    reserved1: u32,
    reserved2: u32,
    executable: bool,
    zero: bool,
}
impl Section {
    fn contains(&self, a: u64) -> bool {
        a >= self.address && a - self.address < self.size
    }
}
#[derive(Clone, Debug)]
struct Symbol {
    name: String,
    address: u64,
    section: u8,
    kind: u8,
    desc: u16,
}
#[derive(Debug)]
struct Image {
    arch: Arch,
    origin: u64,
    length: u64,
    bits: u32,
    be: bool,
    cpu: u32,
    subtype: u32,
    filetype: u32,
    flags: u32,
    sections: Vec<Section>,
    commands: Vec<(u32, Vec<u8>)>,
    code: Option<usize>,
    encrypted: bool,
    inventory: String,
    raw: bool,
}
fn range(o: u64, n: u64, size: u64) -> io::Result<()> {
    if o <= size && n <= size - o {
        Ok(())
    } else {
        Err(bad("Declared range lies outside image"))
    }
}
fn add(a: u64, b: u64) -> io::Result<u64> {
    a.checked_add(b).ok_or_else(|| bad("Address overflow"))
}
fn read(file: &mut File, o: u64, n: usize) -> io::Result<Vec<u8>> {
    if n > 16 * 1024 * 1024 + 32 {
        return Err(bad("Read budget exceeded"));
    }
    file.seek(SeekFrom::Start(o))?;
    let mut b = vec![0; n];
    file.read_exact(&mut b)?;
    Ok(b)
}
fn u32v(b: &[u8], be: bool) -> u32 {
    let a = b[..4].try_into().unwrap();
    if be {
        u32::from_be_bytes(a)
    } else {
        u32::from_le_bytes(a)
    }
}
fn u64v(b: &[u8], be: bool) -> u64 {
    let a = b[..8].try_into().unwrap();
    if be {
        u64::from_be_bytes(a)
    } else {
        u64::from_le_bytes(a)
    }
}
fn name(b: &[u8]) -> String {
    b.iter()
        .take_while(|&&c| c != 0)
        .map(|&c| {
            if c.is_ascii_graphic() || c == b' ' {
                c as char
            } else {
                '?'
            }
        })
        .collect()
}
fn magic(b: &[u8]) -> Option<(u32, bool)> {
    match b.get(..4)? {
        [0xce, 0xfa, 0xed, 0xfe] => Some((32, false)),
        [0xcf, 0xfa, 0xed, 0xfe] => Some((64, false)),
        [0xfe, 0xed, 0xfa, 0xce] => Some((32, true)),
        [0xfe, 0xed, 0xfa, 0xcf] => Some((64, true)),
        _ => None,
    }
}
fn push(out: &mut String, line: &str) {
    if out.len() + line.len() <= TEXT {
        out.push_str(line)
    }
}
fn fingerprint(m: &Metadata) -> (u64, u64, u64, i64, i64, i64, i64) {
    (
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    )
}

fn thin(file: &mut File, origin: u64, length: u64, o: &Options) -> io::Result<Image> {
    range(origin, length, file.metadata()?.len())?;
    if length < 28 {
        return Err(bad("Truncated Mach-O header"));
    }
    let head = read(file, origin, length.min(32) as usize)?;
    let (bits, be) =
        magic(&head).ok_or_else(|| bad("No Mach-O header at selected image offset"))?;
    let header = if bits == 64 { 32 } else { 28 };
    range(0, header, length)?;
    let cpu = u32v(&head[4..], be);
    let arch = Arch::option(&o.architecture).unwrap_or(Arch::cpu(cpu)?);
    if !arch.compatible(cpu) {
        return Err(bad("Selected architecture disagrees with Mach-O CPU"));
    }
    // Supported CPU instruction streams are little endian. Big endian headers are
    // still parsed, but no little-endian instruction decoder is used on those bytes.
    let nc = u32v(&head[16..], be);
    let nb = u32v(&head[20..], be) as u64;
    if nc > 8192 || nb > 16 * 1024 * 1024 {
        return Err(bad("Load-command budget exceeded"));
    }
    range(header, nb, length)?;
    let window = read(file, origin + header, nb as usize)?;
    let mut image = Image {
        arch,
        origin,
        length,
        bits,
        be,
        cpu,
        subtype: u32v(&head[8..], be),
        filetype: u32v(&head[12..], be),
        flags: u32v(&head[24..], be),
        sections: vec![],
        commands: vec![],
        code: None,
        encrypted: false,
        inventory: String::new(),
        raw: false,
    };
    let mut at = 0;
    let mut segments = Vec::new();
    let mut code = None;
    for _ in 0..nc {
        if at + 8 > window.len() {
            return Err(bad("Truncated load command"));
        }
        let cmd = u32v(&window[at..], be);
        let n = u32v(&window[at + 4..], be) as usize;
        if n < 8 || n % (if bits == 64 { 8 } else { 4 }) != 0 || n > window.len() - at {
            return Err(bad("Invalid load-command framing"));
        }
        let p = &window[at..at + n];
        if cmd == 1 || cmd == 0x19 {
            if (bits == 32) != (cmd == 1) {
                return Err(bad("Segment width disagrees with Mach-O header"));
            }
            let fixed = if bits == 64 { 72 } else { 56 };
            let stride = if bits == 64 { 80 } else { 68 };
            if n < fixed {
                return Err(bad("Short segment"));
            }
            let (va, vs, fo, fs, prot, ns) = if bits == 64 {
                (
                    u64v(&p[24..], be),
                    u64v(&p[32..], be),
                    u64v(&p[40..], be),
                    u64v(&p[48..], be),
                    u32v(&p[60..], be),
                    u32v(&p[64..], be),
                )
            } else {
                (
                    u32v(&p[24..], be) as u64,
                    u32v(&p[28..], be) as u64,
                    u32v(&p[32..], be) as u64,
                    u32v(&p[36..], be) as u64,
                    u32v(&p[44..], be),
                    u32v(&p[48..], be),
                )
            };
            if segments.len() >= 64
                || ns > 256
                || image.sections.len() + ns as usize > 256
                || fixed + ns as usize * stride != n
            {
                return Err(bad("Invalid segment section table"));
            }
            range(fo, fs, length)?;
            add(va, vs)?;
            if fs > vs {
                return Err(bad("Segment file extent exceeds virtual extent"));
            }
            for &(a, z, f, l) in &segments {
                if vs > 0 && z > 0 && va < a + z && a < va + vs
                    || fs > 0 && l > 0 && fo < f + l && f < fo + fs
                {
                    return Err(bad("Overlapping segment mappings"));
                }
            }
            segments.push((va, vs, fo, fs));
            for j in 0..ns as usize {
                let s = &p[fixed + j * stride..];
                let (address, size, offset, align, reloc, nreloc, flags, r1, r2) = if bits == 64 {
                    (
                        u64v(&s[32..], be),
                        u64v(&s[40..], be),
                        u32v(&s[48..], be),
                        u32v(&s[52..], be),
                        u32v(&s[56..], be),
                        u32v(&s[60..], be),
                        u32v(&s[64..], be),
                        u32v(&s[68..], be),
                        u32v(&s[72..], be),
                    )
                } else {
                    (
                        u32v(&s[32..], be) as u64,
                        u32v(&s[36..], be) as u64,
                        u32v(&s[40..], be),
                        u32v(&s[44..], be),
                        u32v(&s[48..], be),
                        u32v(&s[52..], be),
                        u32v(&s[56..], be),
                        u32v(&s[60..], be),
                        u32v(&s[64..], be),
                    )
                };
                let zero = matches!(flags & 255, 1 | 0xc | 0x12);
                if address < va || align > 63 {
                    return Err(bad("Invalid section address/alignment"));
                }
                range(address - va, size, vs)?;
                add(address, size)?;
                if !zero {
                    range(offset as u64, size, length)?;
                    if (offset as u64) < fo {
                        return Err(bad("Section precedes its segment"));
                    }
                    range(offset as u64 - fo, size, fs)?;
                    if image.filetype != 1 && address - va != offset as u64 - fo {
                        return Err(bad("Inconsistent section mapping"));
                    }
                }
                range(reloc as u64, nreloc as u64 * 8, length)?;
                let section = Section {
                    name: name(&s[..16]),
                    segment: name(&s[16..32]),
                    address,
                    size,
                    offset: origin + offset as u64,
                    flags,
                    reloc: origin + reloc as u64,
                    nreloc,
                    reserved1: r1,
                    reserved2: r2,
                    executable: !zero
                        && (prot & 4 != 0 || image.filetype == 1)
                        && (flags & 0x80000400 != 0),
                    zero,
                };
                if section.size > 0 {
                    for other in &image.sections {
                        if other.size > 0
                            && (section.address < other.address + other.size
                                && other.address < section.address + section.size
                                || !zero
                                    && !other.zero
                                    && section.offset < other.offset + other.size
                                    && other.offset < section.offset + section.size)
                        {
                            return Err(bad("Overlapping section mappings"));
                        }
                    }
                }
                if section.executable && section.name == "__text" && section.size > 0 {
                    if code.is_some() {
                        return Err(bad("Ambiguous __text mapping"));
                    }
                    code = Some(image.sections.len())
                }
                image.sections.push(section);
            }
        } else if cmd == 2 {
            if n != 24 {
                return Err(bad("Invalid symbol table command"));
            }
            range(
                u32v(&p[8..], be) as u64,
                u32v(&p[12..], be) as u64 * if bits == 64 { 16 } else { 12 },
                length,
            )?;
            range(u32v(&p[16..], be) as u64, u32v(&p[20..], be) as u64, length)?;
        } else if matches!(cmd, 0x21 | 0x2c) {
            if n < 20 {
                return Err(bad("Short encryption command"));
            }
            range(u32v(&p[8..], be) as u64, u32v(&p[12..], be) as u64, length)?;
            image.encrypted |= u32v(&p[16..], be) != 0;
        } else if matches!(cmd, 0x1d | 0x26 | 0x29 | 0x80000033 | 0x80000034) {
            if n != 16 {
                return Err(bad("Short linkedit command"));
            }
            range(u32v(&p[8..], be) as u64, u32v(&p[12..], be) as u64, length)?;
        } else if cmd == 0xb {
            if n != 80 {
                return Err(bad("Short dynamic symbol command"));
            }
            range(
                u32v(&p[56..], be) as u64,
                u32v(&p[60..], be) as u64 * 4,
                length,
            )?;
        } else if matches!(cmd, 0xc | 0xd | 0x80000018 | 0x8000001f | 0x20 | 0x80000023) {
            if n < 24 {
                return Err(bad("Short dylib command"));
            }
            let no = u32v(&p[8..], be) as usize;
            if no < 24 || no >= n || !p[no..].contains(&0) {
                return Err(bad("Invalid dylib name"));
            }
        }
        image.commands.push((cmd, p.to_vec()));
        at += n;
    }
    if at != window.len() {
        return Err(bad("Command count does not consume declared bytes"));
    }
    image.code = code.or_else(|| {
        image
            .sections
            .iter()
            .position(|s| s.executable && s.size > 0)
    });
    if let Some(index) = image.code {
        let section = &image.sections[index];
        if section.offset < origin + header + nb
            || section.address % arch.unit() != 0
            || section.offset % arch.unit() != 0
            || section.size % arch.unit() != 0
        {
            return Err(bad("Invalid instruction section extent/alignment"));
        }
    }
    Ok(image)
}
fn load(file: &mut File, o: &Options) -> io::Result<Image> {
    let size = file.metadata()?.len();
    range(o.offset, 1, size)?;
    let header = read(file, o.offset, (size - o.offset).min(32) as usize)?;
    if o.format != "raw" && magic(&header).is_some() {
        return thin(file, o.offset, size - o.offset, o);
    }
    let prefix = read(file, o.offset, (size - o.offset).min(SCAN as u64) as usize)?;
    if o.format != "raw"
        && prefix.len() >= 8
        && matches!(
            &prefix[..4],
            [0xca, 0xfe, 0xba, 0xbe]
                | [0xca, 0xfe, 0xba, 0xbf]
                | [0xbe, 0xba, 0xfe, 0xca]
                | [0xbf, 0xba, 0xfe, 0xca]
        )
    {
        let be = prefix[0] == 0xca;
        let wide = prefix[3] == 0xbf || prefix[0] == 0xbf;
        let count = u32v(&prefix[4..], be) as usize;
        let stride = if wide { 32 } else { 20 };
        if count == 0 || count > 128 || 8 + count * stride > prefix.len() {
            return Err(bad("Invalid universal slice table"));
        }
        let mut slices = Vec::new();
        let mut inventory = String::new();
        for i in 0..count {
            let p = &prefix[8 + i * stride..];
            let cpu = u32v(p, be);
            let (off, len, align) = if wide {
                (u64v(&p[8..], be), u64v(&p[16..], be), u32v(&p[24..], be))
            } else {
                (
                    u32v(&p[8..], be) as u64,
                    u32v(&p[12..], be) as u64,
                    u32v(&p[16..], be),
                )
            };
            range(off, len, size - o.offset)?;
            if len < 28
                || off < (8 + count * stride) as u64
                || align > 63
                || off % (1u64 << align) != 0
            {
                return Err(bad("Invalid universal slice extent/alignment"));
            }
            for &(_, a, n) in &slices {
                if off < a + n && a < off + len {
                    return Err(bad("Overlapping universal slices"));
                }
            }
            slices.push((cpu, off, len));
            push(
                &mut inventory,
                &format!(
                    "Universal slice {i}: CPU={cpu:#x} image-offset={:#x} size={len:#x}\n",
                    o.offset + off
                ),
            );
        }
        let selected = if let Some(a) = Arch::option(&o.architecture) {
            slices.iter().find(|s| a.compatible(s.0))
        } else {
            [0x0100000c, 0x01000007, 7, 12]
                .iter()
                .find_map(|cpu| slices.iter().find(|s| s.0 == *cpu))
        }
        .ok_or_else(|| bad("No matching supported universal slice"))?;
        let mut image = thin(file, o.offset + selected.1, selected.2, o)?;
        if image.cpu != selected.0 {
            return Err(bad("Universal slice CPU disagrees with its header"));
        }
        image.inventory = inventory;
        return Ok(image);
    }
    if matches!(o.format.as_str(), "auto" | "sepos") {
        let mut images = Vec::new();
        let mut inventory = String::new();
        let mut attempts = 0;
        // Composite SEPI images are inventoried by validated embedded Mach-O headers.
        // A marker/name alone never establishes architecture, plaintext or addresses.
        for at in (0..prefix.len().saturating_sub(28)).step_by(4) {
            if magic(&prefix[at..]).is_none() {
                continue;
            }
            attempts += 1;
            if attempts > 512 {
                push(&mut inventory,"Embedded-header candidate budget reached; select an explicit file offset to continue.\n");
                break;
            }
            if let Ok(image) = thin(file, o.offset + at as u64, size - o.offset - at as u64, o) {
                let Some(index) = image.code else {
                    continue;
                };
                push(
                    &mut inventory,
                    &format!(
                        "Embedded Mach-O: image-offset={:#x} {} code={:#x}..{:#x}\n",
                        image.origin,
                        image.arch.name(),
                        image.sections[index].address,
                        image.sections[index].address + image.sections[index].size
                    ),
                );
                images.push(image);
                if images.len() >= 128 {
                    break;
                }
            }
        }
        if !images.is_empty() {
            let mut image = images.remove(0);
            image.inventory=format!("Composite scan limited to first {SCAN} bytes; first validated image selected. Import again with an explicit image offset to select another.\n{inventory}");
            return Ok(image);
        }
    }
    if matches!(o.format.as_str(), "macho" | "sepos") {
        return Err(bad(
            "No validated Mach-O image found; choose its file offset or explicit raw architecture",
        ));
    }
    if o.format != "raw"
        && (prefix
            .iter()
            .take(128)
            .copied()
            .collect::<Vec<_>>()
            .windows(4)
            .any(|w| w == b"IM4P" || w == b"IMG4")
            || prefix.starts_with(b"bvx2")
            || prefix.starts_with(b"bvxn")
            || prefix.starts_with(b"complzss"))
    {
        return Err(bad("Encoded/compressed IMG4/IM4P firmware must be unpacked to a plaintext image before importing"));
    }
    let arch=Arch::option(&o.architecture).ok_or_else(||bad("Raw SecureROM/iBoot/firmware requires an explicit architecture and load address; encoded/encrypted containers must be unpacked first"))?;
    let length = size - o.offset;
    if o.base % arch.unit() != 0 || o.offset % arch.unit() != 0 || length % arch.unit() != 0 {
        return Err(bad("Raw image does not satisfy instruction alignment"));
    }
    add(o.base, length)?;
    Ok(Image {
        arch,
        origin: o.offset,
        length,
        bits: if arch.width() == 8 { 64 } else { 32 },
        be: false,
        cpu: 0,
        subtype: 0,
        filetype: 0,
        flags: 0,
        sections: vec![Section {
            name: "raw-code".into(),
            segment: "analyst-mapping".into(),
            address: o.base,
            size: length,
            offset: o.offset,
            flags: 0,
            reloc: 0,
            nreloc: 0,
            reserved1: 0,
            reserved2: 0,
            executable: true,
            zero: false,
        }],
        commands: vec![],
        code: Some(0),
        encrypted: false,
        inventory:
            "Raw mapping is analyst supplied; code/data boundaries and entry point are unknown.\n"
                .into(),
        raw: true,
    })
}
extern "C" {
    fn hs_decoder_new(arch: u32) -> *mut c_void;
    fn hs_decoder_instruction(
        ctx: *mut c_void,
        bytes: *const u8,
        len: usize,
        pc: u64,
        out: *mut c_char,
        cap: usize,
    ) -> usize;
    fn hs_decoder_free(ctx: *mut c_void);
    fn re_open_source(path: *const c_char) -> i32;
    fn re_verify_fd(
        fd: i32,
        digest: *const c_char,
        size: u64,
        error: *mut c_char,
        cap: usize,
    ) -> i32;
}
struct Decoder(*mut c_void);
impl Decoder {
    fn new(a: Arch) -> io::Result<Self> {
        let p = unsafe { hs_decoder_new(a as u32) };
        if p.is_null() {
            Err(bad("LLVM disassembler unavailable for this architecture. Install Xcode, Command Line Tools or LLVM."))
        } else {
            Ok(Self(p))
        }
    }
    fn instruction(&self, b: &[u8], pc: u64, a: Arch) -> (usize, String, bool) {
        let mut out = [0 as c_char; 512];
        let n = unsafe {
            hs_decoder_instruction(self.0, b.as_ptr(), b.len(), pc, out.as_mut_ptr(), out.len())
        };
        if n == 0 {
            let n = (a.unit() as usize).min(b.len());
            (
                n,
                format!(
                    ".byte {} ; undecoded",
                    b[..n]
                        .iter()
                        .map(|v| format!("0x{v:02x}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                false,
            )
        } else {
            (
                n,
                unsafe { CStr::from_ptr(out.as_ptr()) }
                    .to_string_lossy()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
                true,
            )
        }
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe { hs_decoder_free(self.0) }
    }
}
fn code_ready(image: &Image) -> io::Result<()> {
    if image.code.is_none() {
        return Err(bad(
            "No file-backed instruction section; metadata remains available",
        ));
    }
    if image.encrypted {
        return Err(bad(
            "Code is declared encrypted; metadata remains available",
        ));
    }
    if image.be {
        return Err(bad("Big-endian Mach-O metadata parsed; instruction decoding for this byte order is unavailable"));
    }
    Ok(())
}
fn data_ranges(file: &mut File, i: &Image) -> io::Result<Vec<(u64, u64)>> {
    let mut out = vec![];
    for (cmd, p) in &i.commands {
        if *cmd != 0x29 {
            continue;
        }
        let n = u32v(&p[12..], i.be) as usize;
        if n > 8 * 1024 * 1024 || n % 8 != 0 {
            return Err(bad("Invalid data-in-code table budget"));
        }
        let b = read(file, i.origin + u32v(&p[8..], i.be) as u64, n)?;
        for e in b.chunks_exact(8) {
            let off = u32v(e, i.be) as u64;
            let len = if i.be {
                u16::from_be_bytes(e[4..6].try_into().unwrap())
            } else {
                u16::from_le_bytes(e[4..6].try_into().unwrap())
            } as u64;
            range(off, len, i.length)?;
            out.push((i.origin + off, i.origin + off + len));
        }
    }
    Ok(out)
}
fn page(file: &mut File, i: &Image, start: u64) -> io::Result<(String, usize)> {
    code_ready(i)?;
    let s = &i.sections[i.code.ok_or_else(|| bad("No instruction section"))?];
    let unit = i.arch.unit();
    let delta = start
        .checked_mul(unit)
        .ok_or_else(|| bad("Page overflow"))?;
    if delta >= s.size {
        return Err(bad("Page starts outside instruction section"));
    }
    let budget = (s.size - delta).min(256 * unit) as usize;
    let b = read(
        file,
        s.offset + delta,
        (s.size - delta).min((budget + 16) as u64) as usize,
    )?;
    let decoder = Decoder::new(i.arch)?;
    let ranges = data_ranges(file, i)?;
    let mut out = format!(
        "{} — VA (unslid) / file offset / bytes / instruction\n",
        i.arch.name()
    );
    let mut at = 0;
    while at < budget {
        let pos = s.offset + delta + at as u64;
        let pc = s.address + delta + at as u64;
        let (n, text, _) = if ranges.iter().any(|&(lo, hi)| pos < hi && pos + unit > lo) {
            (unit as usize, ".data-in-code".into(), false)
        } else {
            decoder.instruction(&b[at..], pc, i.arch)
        };
        if at + n > budget && at > 0 {
            break;
        }
        let text = if ranges
            .iter()
            .any(|&(lo, hi)| pos < hi && pos + n as u64 > lo)
        {
            ".data-in-code".to_string()
        } else {
            text
        };
        let text = if let Some((target, kind)) = branch(i.arch, &b[at..at + n], pc) {
            format!("{text} ; {kind} target=0x{target:x}")
        } else {
            text
        };
        let bytes = b[at..at + n]
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect::<String>();
        push(
            &mut out,
            &format!("0x{pc:016x}  0x{pos:016x}  {bytes:<30} {text}\n"),
        );
        at += n;
    }
    push(&mut out,"\nLinear decode from the selected address; undecoded bytes and data-in-code records are barriers.\n");
    Ok((out, at / unit as usize))
}
pub fn describe(
    file: &mut File,
    o: &Options,
) -> io::Result<(String, String, String, String, String, String)> {
    let i = match load(file, o) {
        Ok(image) => image,
        Err(error) => {
            if let Some((kind, structure)) = inspect_firmware_container(file)? {
                return Ok((
                    kind,
                    "Container".into(),
                    "checked".into(),
                    structure,
                    "unavailable".into(),
                    "Instruction analysis requires a validated plaintext payload; container unpacking is not performed by HindSight.".into(),
                ));
            }
            return Err(error);
        }
    };
    let structure = metadata(&i);
    let (status, preview) = match page(file, &i, 0) {
        Ok((text, _)) => ("preview".into(), text),
        Err(e) => ("metadata-only".into(), e.to_string()),
    };
    Ok((
        if i.raw {
            format!("{} raw image", o.format)
        } else {
            format!(
                "Mach-O {} {}",
                i.bits,
                if i.be { "big-endian" } else { "little-endian" }
            )
        },
        i.arch.name().into(),
        "checked".into(),
        structure,
        status,
        preview,
    ))
}

/// Inventory recognizable firmware-container markers without interpreting or
/// unpacking their payload. This keeps capture useful for triage while making
/// the boundary explicit: offsets are observations, not validated ASN.1 fields.
fn inspect_firmware_container(file: &mut File) -> io::Result<Option<(String, String)>> {
    const BUDGET: usize = 4 * 1024 * 1024;
    let size = file.metadata()?.len();
    let bytes = read(file, 0, size.min(BUDGET as u64) as usize)?;
    let marker = |needle: &[u8]| -> Vec<usize> {
        bytes
            .windows(needle.len())
            .enumerate()
            .filter_map(|(offset, window)| (window == needle).then_some(offset))
            .take(32)
            .collect()
    };
    let img4 = marker(b"IMG4");
    let im4p = marker(b"IM4P");
    if img4.is_empty() && im4p.is_empty() {
        return Ok(None);
    }
    let bvx2 = marker(b"bvx2");
    let bvxn = marker(b"bvxn");
    let complzss = marker(b"complzss");
    let mut out = format!(
        "Container marker inventory (first {:#x} bytes; at most 32 matches per marker)\n",
        bytes.len()
    );
    if !img4.is_empty() {
        push(&mut out, &format!("IMG4 marker offsets: {:?}\n", img4));
    }
    if !im4p.is_empty() {
        push(
            &mut out,
            &format!("IM4P payload marker offsets: {:?}\n", im4p),
        );
    }
    if !bvx2.is_empty() {
        push(
            &mut out,
            &format!("bvx2 compression marker offsets: {:?}\n", bvx2),
        );
    }
    if !bvxn.is_empty() {
        push(
            &mut out,
            &format!("bvxn compression marker offsets: {:?}\n", bvxn),
        );
    }
    if !complzss.is_empty() {
        push(
            &mut out,
            &format!("complzss compression marker offsets: {:?}\n", complzss),
        );
    }
    if bytes.first() == Some(&0x30) {
        push(&mut out, "DER-like sequence prefix observed at file offset 0; complete ASN.1 structure is not decoded.\n");
    }
    push(&mut out, &format!(
        "File size={size:#x}; scan budget={BUDGET:#x}. Payload boundaries, encryption and compression validity are not established.\n"
    ));
    let kind = if !img4.is_empty() {
        "IMG4 firmware container"
    } else {
        "IM4P firmware payload"
    };
    Ok(Some((kind.into(), out)))
}
fn verified(path: &str, digest: &str, size: &str, id: &str) -> io::Result<(File, Metadata, u64)> {
    if path.len() > 16384
        || !std::path::Path::new(path).is_absolute()
        || digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || id.len() != 36
        || !id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
    {
        return Err(bad("Invalid source identity"));
    }
    let size: u64 = size.parse().map_err(|_| bad("Invalid source size"))?;
    if size == 0 || size > 8 * 1024 * 1024 * 1024 {
        return Err(bad("Source size budget"));
    }
    let p = CString::new(path)?;
    let d = CString::new(digest)?;
    let fd = unsafe { re_open_source(p.as_ptr()) };
    if fd < 0 {
        return Err(bad(
            "Cannot open captured regular file without following symlinks",
        ));
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let before = file.metadata()?;
    let mut e = [0 as c_char; 512];
    if unsafe { re_verify_fd(fd, d.as_ptr(), size, e.as_mut_ptr(), e.len()) } != 0 {
        return Err(bad(&unsafe { CStr::from_ptr(e.as_ptr()) }.to_string_lossy()));
    }
    Ok((file, before, size))
}
pub fn request(
    path: &str,
    digest: &str,
    size: &str,
    start: &str,
    id: &str,
    o: &Options,
) -> io::Result<String> {
    let (mut file, before, _) = verified(path, digest, size, id)?;
    let i = load(&mut file, o)?;
    let start: u64 = start.parse().map_err(|_| bad("Invalid page start"))?;
    let (text, count) = page(&mut file, &i, start)?;
    if fingerprint(&before) != fingerprint(&file.metadata()?) {
        return Err(bad("Captured object changed during page request"));
    }
    let s = &i.sections[i.code.ok_or_else(|| bad("No instruction section"))?];
    Ok(format!("{{\"protocol\":1,\"kind\":\"page_result\",\"request_id\":{},\"source_sha256\":{},\"start_word\":{},\"total_words\":{},\"section_address\":{},\"section_offset\":{},\"word_count\":{},\"unit_bytes\":{},\"text\":{}}}",super::quote(id),super::quote(digest),super::quote(&start.to_string()),super::quote(&(s.size/i.arch.unit()).to_string()),super::quote(&s.address.to_string()),super::quote(&s.offset.to_string()),count,i.arch.unit(),super::quote(&text)))
}
fn metadata(i: &Image) -> String {
    let mut out=format!("Header: {} bits, {} endian; CPU={:#x} subtype={:#x} filetype={} flags={:#x}\nSelected image offset={:#x}; image bound={:#x}; decoder={}\n",i.bits,if i.be{"big"}else{"little"},i.cpu,i.subtype,i.filetype,i.flags,i.origin,i.length,i.arch.name());
    push(&mut out, &i.inventory);
    if i.cpu == 12 {
        push(&mut out,"ARM32 mode: Thumb-2 is the import default. Override ARM/Thumb for mixed-mode firmware; section flags alone do not establish instruction mode.\n")
    }
    for (cmd, p) in &i.commands {
        push(
            &mut out,
            &format!("Load command {cmd:#010x}, {} bytes\n", p.len()),
        );
        if matches!(
            *cmd,
            0xc | 0xd | 0x80000018 | 0x8000001f | 0x20 | 0x80000023
        ) {
            push(
                &mut out,
                &format!("  dylib={}\n", name(&p[u32v(&p[8..], i.be) as usize..])),
            );
        }
    }
    for s in &i.sections {
        push(
            &mut out,
            &format!(
                "Section {}.{} VA={:#x} size={:#x} file={:#x} flags={:#x} {} {}\n",
                s.segment,
                s.name,
                s.address,
                s.size,
                s.offset,
                s.flags,
                if s.zero { "zero-fill" } else { "file-backed" },
                if s.executable { "instructions" } else { "data" }
            ),
        );
    }
    if i.encrypted {
        push(
            &mut out,
            "Encryption metadata is nonzero: instruction analysis disabled.\n",
        )
    }
    push(&mut out,"Bounds, selected command payloads and nonoverlapping mappings checked; unknown command payloads and code signatures are not validated.\n");
    out
}
fn symbols(file: &mut File, i: &Image) -> io::Result<Vec<Symbol>> {
    let mut out = vec![];
    let mut name_bytes = 0usize;
    for (cmd, p) in &i.commands {
        if *cmd != 2 {
            continue;
        }
        let count = u32v(&p[12..], i.be) as usize;
        let bytes = u32v(&p[20..], i.be) as usize;
        if count > 200_000 || bytes > 8 * 1024 * 1024 {
            return Err(bad("Symbol/string-table budget exceeded"));
        }
        let strings = read(file, i.origin + u32v(&p[16..], i.be) as u64, bytes)?;
        let stride = if i.bits == 64 { 16 } else { 12 };
        let table = read(file, i.origin + u32v(&p[8..], i.be) as u64, count * stride)?;
        for e in table.chunks_exact(stride) {
            let index = u32v(e, i.be) as usize;
            let kind = e[4];
            let section = e[5];
            if index >= strings.len() && index != 0 {
                return Err(bad("Symbol name outside string table"));
            }
            if kind & 0xe0 != 0 {
                out.push(Symbol {
                    name: String::new(),
                    address: 0,
                    section,
                    kind,
                    desc: 0,
                });
                continue;
            }
            let n = if index == 0 {
                String::new()
            } else {
                let tail = &strings[index..];
                let end = tail
                    .iter()
                    .position(|&b| b == 0)
                    .ok_or_else(|| bad("Unterminated symbol name"))?;
                name(&tail[..end.min(1024)])
            };
            name_bytes += n.len();
            if name_bytes > 16 * 1024 * 1024 {
                return Err(bad("Aggregate symbol-name budget exceeded"));
            }
            let address = if i.bits == 64 {
                u64v(&e[8..], i.be)
            } else {
                u32v(&e[8..], i.be) as u64
            };
            let desc = if i.be {
                u16::from_be_bytes(e[6..8].try_into().unwrap())
            } else {
                u16::from_le_bytes(e[6..8].try_into().unwrap())
            };
            if kind & 0xe == 0xe && (section == 0 || section as usize > i.sections.len()) {
                return Err(bad("Symbol references invalid section"));
            }
            out.push(Symbol {
                name: n,
                address,
                section,
                kind,
                desc,
            });
        }
    }
    Ok(out)
}
fn uleb(b: &[u8], at: &mut usize) -> io::Result<u64> {
    let mut v = 0;
    for shift in (0..=63).step_by(7) {
        let c = *b.get(*at).ok_or_else(|| bad("Truncated ULEB128"))?;
        *at += 1;
        if shift == 63 && c & 0x7e != 0 {
            return Err(bad("ULEB128 overflow"));
        }
        v |= ((c & 0x7f) as u64) << shift;
        if c & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err(bad("ULEB128 overflow"))
}
fn function_starts(file: &mut File, i: &Image) -> io::Result<BTreeSet<u64>> {
    let mut out = BTreeSet::new();
    let base = i
        .commands
        .iter()
        .find_map(|(cmd, p)| {
            if matches!(*cmd, 1 | 0x19) && name(&p[8..24]) == "__TEXT" {
                Some(if *cmd == 0x19 {
                    u64v(&p[24..], i.be)
                } else {
                    u32v(&p[24..], i.be) as u64
                })
            } else {
                None
            }
        })
        .unwrap_or(0);
    for (cmd, p) in &i.commands {
        if *cmd != 0x26 {
            continue;
        }
        let n = u32v(&p[12..], i.be) as usize;
        if n > 8 * 1024 * 1024 {
            return Err(bad("Function-start table budget"));
        }
        let b = read(file, i.origin + u32v(&p[8..], i.be) as u64, n)?;
        let mut at = 0;
        let mut address = base;
        let mut terminated = b.is_empty();
        while at < b.len() && out.len() < 200_000 {
            let delta = uleb(&b, &mut at)?;
            if delta == 0 {
                terminated = true;
                break;
            }
            address = add(address, delta)?;
            if i.sections
                .iter()
                .any(|s| s.executable && s.contains(address))
            {
                out.insert(address);
            }
        }
        if !terminated {
            return Err(bad("Unterminated or over-budget function-start table"));
        }
    }
    Ok(out)
}
fn entry_points(i: &Image) -> io::Result<Vec<u64>> {
    let mut out = vec![];
    for (cmd, p) in &i.commands {
        if *cmd == 0x80000028 {
            if p.len() != 24 {
                return Err(bad("Invalid LC_MAIN command"));
            }
            let off = u64v(&p[8..], i.be);
            range(off, 1, i.length)?;
            let position = i.origin + off;
            if let Some(s) = i
                .sections
                .iter()
                .find(|s| s.executable && position >= s.offset && position - s.offset < s.size)
            {
                out.push(s.address + position - s.offset);
            }
        } else if matches!(*cmd, 4 | 5) {
            let mut at = 8;
            while at < p.len() {
                if p.len() - at < 8 {
                    return Err(bad("Truncated thread-state header"));
                }
                let flavor = u32v(&p[at..], i.be);
                let count = u32v(&p[at + 4..], i.be) as usize;
                at += 8;
                if count > p.len().saturating_sub(at) / 4 {
                    return Err(bad("Thread-state count exceeds command"));
                }
                let state = &p[at..at + count * 4];
                let field = match (i.cpu, flavor) {
                    (12, 1) => Some((60, 4)),
                    (0x0100000c, 6) => Some((256, 8)),
                    (7, 1) => Some((40, 4)),
                    (0x01000007, 4) => Some((128, 8)),
                    _ => None,
                };
                if let Some((offset, width)) = field {
                    if offset + width > state.len() {
                        return Err(bad("Truncated architecture thread state"));
                    }
                    let address = if width == 8 {
                        u64v(&state[offset..], i.be)
                    } else {
                        u32v(&state[offset..], i.be) as u64
                    };
                    let address = if i.cpu == 12 { address & !1 } else { address };
                    if i.sections
                        .iter()
                        .any(|s| s.executable && s.contains(address))
                    {
                        out.push(address);
                    }
                }
                at += count * 4;
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}
fn strings(file: &mut File, i: &Image) -> io::Result<(String, bool)> {
    let mut out = String::new();
    let mut remaining = SCAN;
    let mut limited = false;
    for s in i
        .sections
        .iter()
        .filter(|s| !s.zero && (!s.executable || i.raw))
    {
        if remaining == 0 {
            limited = true;
            break;
        }
        let n = (s.size.min(remaining as u64)) as usize;
        limited |= (n as u64) < s.size;
        remaining -= n;
        let b = read(file, s.offset, n)?;
        let mut at = 0;
        while at < b.len() {
            let start = at;
            while at < b.len() && (b[at].is_ascii_graphic() || b[at] == b' ') {
                at += 1
            }
            if at - start >= 4 {
                let text = name(&b[start..at.min(start + 512)]);
                push(
                    &mut out,
                    &format!(
                        "0x{:016x} file={:#x} ASCII {:?} section={}\n",
                        s.address + start as u64,
                        s.offset + start as u64,
                        text,
                        s.name
                    ),
                );
            }
            at += 1;
        }
        let mut at = 0;
        while at + 1 < b.len() {
            let start = at;
            let mut units = Vec::new();
            let mut terminated = false;
            while at + 1 < b.len() && units.len() < 512 {
                let unit = if i.be {
                    u16::from_be_bytes([b[at], b[at + 1]])
                } else {
                    u16::from_le_bytes([b[at], b[at + 1]])
                };
                if unit == 0 {
                    terminated = true;
                    break;
                }
                units.push(unit);
                at += 2;
            }
            let mut emitted = false;
            let preceded_by_terminator = start > 0 && b[start - 1] == 0;
            let plausible_start = preceded_by_terminator
                || b[start] == 0
                || b[start + 1] == 0
                || b[start] >= 0x80
                || b[start + 1] >= 0x80;
            let boundary = start == 0 || b[start - 1] == 0 || !b[start - 1].is_ascii_graphic();
            if plausible_start && boundary && terminated && units.len() >= 4 {
                if let Ok(text) = String::from_utf16(&units) {
                    let printable = text
                        .chars()
                        .all(|character| !character.is_control() || character.is_whitespace());
                    if printable {
                        let encoding = if i.be { "UTF16-BE" } else { "UTF16-LE" };
                        push(
                            &mut out,
                            &format!(
                                "0x{:016x} file={:#x} {} {:?} section={}\n",
                                s.address + start as u64,
                                s.offset + start as u64,
                                encoding,
                                text,
                                s.name
                            ),
                        );
                        emitted = true;
                    }
                }
            }
            at = if emitted {
                start + units.len() * 2 + 2
            } else {
                start + 1
            };
        }
    }
    Ok((out, limited))
}
fn relocations(file: &mut File, i: &Image, symbols: &[Symbol]) -> io::Result<String> {
    if i.be {
        return Ok("Big-endian relocation bitfields are not decoded by this backend.".into());
    }
    let mut out = String::new();
    let mut budget = 100_000;
    for s in &i.sections {
        if s.nreloc as usize > budget {
            return Err(bad("Relocation table budget"));
        }
        budget -= s.nreloc as usize;
        let b = read(file, s.reloc, s.nreloc as usize * 8)?;
        for e in b.chunks_exact(8) {
            let a = u32v(e, i.be);
            let word = u32v(&e[4..], i.be);
            if a & 0x80000000 != 0 {
                push(
                    &mut out,
                    &format!(
                        "{} scattered-relocation address={:#x} value={word:#x}\n",
                        s.name,
                        a & 0xffffff
                    ),
                );
                continue;
            }
            let offset = a as i32;
            if offset < 0 {
                push(
                    &mut out,
                    &format!(
                        "{} relocation special-address={offset} raw={word:#x}\n",
                        s.name
                    ),
                );
                continue;
            }
            if offset as u64 >= s.size {
                return Err(bad("Relocation site outside section"));
            }
            let index = word & 0xffffff;
            let external = word >> 27 & 1 != 0;
            let target = if external {
                symbols
                    .get(index as usize)
                    .map(|s| s.name.as_str())
                    .ok_or_else(|| bad("Relocation symbol index outside nlist"))?
            } else {
                "section-relative"
            };
            push(
                &mut out,
                &format!(
                    "0x{:016x} relocation type={} width={} pc-relative={} symbol={} index={}\n",
                    s.address + offset as u64,
                    word >> 28,
                    1 << (word >> 25 & 3),
                    word >> 24 & 1,
                    target,
                    index
                ),
            );
        }
    }
    Ok(out)
}
fn pointer_tables(file: &mut File, i: &Image) -> io::Result<(String, String, bool)> {
    let mut pointers = String::new();
    let mut tables = String::new();
    let width = i.arch.width();
    let mut budget = SCAN;
    let mut limited = false;
    for s in i.sections.iter().filter(|s| !s.zero && !s.executable) {
        if budget == 0 {
            limited = true;
            break;
        }
        let n = s.size.min(budget as u64) as usize;
        limited |= (n as u64) < s.size;
        budget -= n;
        let b = read(file, s.offset, n)?;
        let mut run = Vec::new();
        let flush = |run: &mut Vec<(u64, u64, bool)>, tables: &mut String| {
            if run.len() >= 3 {
                push(tables,&format!("0x{:016x} validated executable-target run count={} targets={} modes={} ; dispatch use and index bounds remain unproven\n",run[0].0,run.len(),run.iter().take(16).map(|r|format!("{:#x}",r.1)).collect::<Vec<_>>().join(","),run.iter().take(16).map(|r|if r.2{"Thumb"}else{"ARM"}).collect::<Vec<_>>().join(",")))
            }
            run.clear();
        };
        let skip = ((width as u64 - s.address % width as u64) % width as u64) as usize;
        for (index, e) in b.get(skip..).unwrap_or(&[]).chunks_exact(width).enumerate() {
            let value = if width == 8 {
                u64v(e, i.be)
            } else {
                u32v(e, i.be) as u64
            };
            let normalized = if i.cpu == 12 { value & !1 } else { value };
            let site = s.address + (skip + index * width) as u64;
            if let Some(target) = i.sections.iter().find(|s| s.contains(normalized)) {
                push(
                    &mut pointers,
                    &format!(
                        "0x{site:016x} -> {value:#018x} {}.{}{}\n",
                        target.segment,
                        target.name,
                        if value != normalized {
                            " Thumb-tag"
                        } else {
                            ""
                        }
                    ),
                );
                if target.executable {
                    run.push((site, normalized, value != normalized));
                } else {
                    flush(&mut run, &mut tables)
                }
            } else {
                flush(&mut run, &mut tables)
            }
        }
        flush(&mut run, &mut tables);
    }
    Ok((pointers, tables, limited))
}
fn indirect_symbols(file: &mut File, i: &Image, symbols: &[Symbol]) -> io::Result<String> {
    let mut out = String::new();
    for (cmd, p) in &i.commands {
        if *cmd != 0xb {
            continue;
        }
        let count = u32v(&p[60..], i.be) as usize;
        if count > 200_000 {
            return Err(bad("Indirect symbol table budget"));
        }
        let b = read(file, i.origin + u32v(&p[56..], i.be) as u64, count * 4)?;
        for s in &i.sections {
            let width = match s.flags & 255 {
                6 | 7 | 0x10 => i.arch.width(),
                8 => s.reserved2 as usize,
                _ => continue,
            };
            if width == 0 {
                return Err(bad("Zero-width symbol stub"));
            }
            let n = (s.size / width as u64) as usize;
            let start = s.reserved1 as usize;
            if start > count || n > count - start {
                return Err(bad("Indirect section range exceeds table"));
            }
            for j in 0..n {
                let index = u32v(&b[(start + j) * 4..], i.be);
                let text = if index & 0xc0000000 != 0 {
                    "local/absolute"
                } else {
                    symbols
                        .get(index as usize)
                        .map(|s| s.name.as_str())
                        .ok_or_else(|| bad("Indirect symbol index outside nlist"))?
                };
                push(
                    &mut out,
                    &format!(
                        "0x{:016x} {} symbol={}\n",
                        s.address + j as u64 * width as u64,
                        s.name,
                        text
                    ),
                );
            }
        }
    }
    Ok(out)
}
fn exports(file: &mut File, i: &Image) -> io::Result<(String, Vec<(u64, String)>)> {
    let mut out = String::new();
    let mut names = vec![];
    let base = i
        .sections
        .iter()
        .find(|s| s.segment == "__TEXT")
        .and_then(|s| s.address.checked_sub(s.offset - i.origin))
        .unwrap_or(0);
    for (cmd, p) in &i.commands {
        let payload = if *cmd == 0x80000033 {
            Some((u32v(&p[8..], i.be), u32v(&p[12..], i.be)))
        } else if matches!(*cmd, 0x22 | 0x80000022) && p.len() >= 48 {
            Some((u32v(&p[40..], i.be), u32v(&p[44..], i.be)))
        } else {
            None
        };
        let (off, n) = match payload {
            Some(x) => x,
            None => continue,
        };
        if n == 0 {
            continue;
        }
        if n > 8 * 1024 * 1024 {
            return Err(bad("Export trie budget"));
        }
        range(off as u64, n as u64, i.length)?;
        let b = read(file, i.origin + off as u64, n as usize)?;
        let mut pending = vec![(0usize, String::new(), 0usize)];
        let mut seen = BTreeSet::new();
        while let Some((node, prefix, depth)) = pending.pop() {
            if depth > 64 || seen.len() >= 8192 {
                return Err(bad("Export trie traversal budget"));
            }
            if !seen.insert(node) {
                return Err(bad("Cyclic/shared export trie node"));
            }
            let mut at = node;
            let terminal = uleb(&b, &mut at)? as usize;
            if terminal > b.len().saturating_sub(at) {
                return Err(bad("Export terminal outside trie"));
            }
            let end = at + terminal;
            if terminal > 0 {
                let flags = uleb(&b[..end], &mut at)?;
                if flags & 8 != 0 {
                    let ordinal = uleb(&b[..end], &mut at)?;
                    push(
                        &mut out,
                        &format!("name={} reexport ordinal={}\n", prefix, ordinal),
                    );
                } else {
                    let value = uleb(&b[..end], &mut at)?;
                    if flags & 3 == 0 || flags & 3 == 2 {
                        let address = if flags & 3 == 2 {
                            value
                        } else {
                            add(base, value)?
                        };
                        if i.sections
                            .iter()
                            .any(|s| s.executable && s.contains(address))
                        {
                            names.push((address, prefix.clone()));
                        }
                    }
                    push(&mut out,&format!("name={} export-value={value:#x} flags={flags:#x} (image-relative unless absolute)\n",prefix));
                }
            }
            at = end;
            let children = *b.get(at).ok_or_else(|| bad("Missing export child count"))?;
            at += 1;
            for _ in 0..children {
                let tail = b.get(at..).ok_or_else(|| bad("Export edge outside trie"))?;
                let len = tail
                    .iter()
                    .position(|&b| b == 0)
                    .ok_or_else(|| bad("Unterminated export edge"))?;
                if len == 0 || prefix.len() + len > 1024 {
                    return Err(bad("Export name budget"));
                }
                let edge = name(&tail[..len]);
                at += len + 1;
                let child = uleb(&b, &mut at)? as usize;
                if child >= b.len() {
                    return Err(bad("Export child outside trie"));
                }
                if pending.len() >= 8192 {
                    return Err(bad("Export trie pending-node budget"));
                }
                pending.push((child, format!("{prefix}{edge}"), depth + 1));
            }
        }
    }
    Ok((out, names))
}
#[derive(Debug)]
struct Instruction {
    address: u64,
    offset: u64,
    bytes: Vec<u8>,
    text: String,
    known: bool,
    branch: Option<(u64, &'static str)>,
    terminal: bool,
}
fn signed(v: u64, bits: u32) -> i64 {
    ((v << (64 - bits)) as i64) >> (64 - bits)
}
fn relative(pc: u64, delta: i64) -> Option<u64> {
    if delta < 0 {
        pc.checked_sub(delta.unsigned_abs())
    } else {
        pc.checked_add(delta as u64)
    }
}
fn branch(a: Arch, b: &[u8], pc: u64) -> Option<(u64, &'static str)> {
    let found = match a {
        Arch::A64 if b.len() == 4 => {
            let w = u32v(b, false);
            if w & 0x7c000000 == 0x14000000 {
                Some((
                    pc,
                    signed((w & 0x3ffffff) as u64, 26) * 4,
                    if w >> 31 != 0 { "call" } else { "branch" },
                ))
            } else if w & 0xff000010 == 0x54000000 || w & 0x7e000000 == 0x34000000 {
                Some((
                    pc,
                    signed(((w >> 5) & 0x7ffff) as u64, 19) * 4,
                    "conditional",
                ))
            } else if w & 0x7e000000 == 0x36000000 {
                Some((
                    pc,
                    signed(((w >> 5) & 0x3fff) as u64, 14) * 4,
                    "conditional",
                ))
            } else {
                None
            }
        }
        Arch::Arm if b.len() == 4 => {
            let w = u32v(b, false);
            if w & 0x0e000000 == 0x0a000000 && w >> 28 != 15 {
                Some((
                    pc.checked_add(8)?,
                    signed((w & 0xffffff) as u64, 24) * 4,
                    if w & 0x01000000 != 0 {
                        "call"
                    } else if w >> 28 == 14 {
                        "branch"
                    } else {
                        "conditional"
                    },
                ))
            } else {
                None
            }
        }
        Arch::Thumb => {
            let h = u16::from_le_bytes(b.get(..2)?.try_into().ok()?);
            if b.len() == 2 && h & 0xf800 == 0xe000 {
                Some((
                    pc.checked_add(4)?,
                    signed((h & 0x7ff) as u64, 11) * 2,
                    "branch",
                ))
            } else if b.len() == 2 && h & 0xf000 == 0xd000 && h & 0xf00 < 0xe00 {
                Some((
                    pc.checked_add(4)?,
                    signed((h & 255) as u64, 8) * 2,
                    "conditional",
                ))
            } else if b.len() == 4 {
                let h2 = u16::from_le_bytes(b[2..4].try_into().ok()?);
                if h & 0xf800 == 0xf000 && h2 & 0xd000 == 0xd000 {
                    let s = (h >> 10) & 1;
                    let i1 = 1 ^ ((h2 >> 13) & 1) ^ s;
                    let i2 = 1 ^ ((h2 >> 11) & 1) ^ s;
                    let v = ((s as u64) << 24)
                        | ((i1 as u64) << 23)
                        | ((i2 as u64) << 22)
                        | (((h & 0x3ff) as u64) << 12)
                        | (((h2 & 0x7ff) as u64) << 1);
                    Some((pc.checked_add(4)?, signed(v, 25), "call"))
                } else {
                    None
                }
            } else {
                None
            }
        }
        Arch::X86 | Arch::X64 => {
            let n = b.len();
            if n == 5 && matches!(b[0], 0xe8 | 0xe9) {
                Some((
                    pc.checked_add(n as u64)?,
                    i32::from_le_bytes(b[1..5].try_into().ok()?) as i64,
                    if b[0] == 0xe8 { "call" } else { "branch" },
                ))
            } else if n == 2
                && (b[0] == 0xeb || (0x70..=0x7f).contains(&b[0]) || (0xe0..=0xe3).contains(&b[0]))
            {
                Some((
                    pc.checked_add(2)?,
                    b[1] as i8 as i64,
                    if b[0] == 0xeb {
                        "branch"
                    } else {
                        "conditional"
                    },
                ))
            } else if n == 6 && b[0] == 0x0f && (0x80..=0x8f).contains(&b[1]) {
                Some((
                    pc.checked_add(6)?,
                    i32::from_le_bytes(b[2..6].try_into().ok()?) as i64,
                    "conditional",
                ))
            } else {
                None
            }
        }
        _ => None,
    };
    let (base, delta, kind) = found?;
    relative(base, delta).map(|target| (target, kind))
}
fn data_reference(a: Arch, b: &[u8], pc: u64) -> Option<u64> {
    match a {
        Arch::A64 if b.len() == 4 => {
            let w = u32v(b, false);
            if w & 0x9f000000 == 0x10000000 || w & 0x9f000000 == 0x90000000 {
                let imm = signed(
                    (((w >> 5) & 0x7ffff) as u64) << 2 | ((w >> 29) & 3) as u64,
                    21,
                );
                if w >> 31 != 0 {
                    relative(pc & !0xfff, imm * 4096)
                } else {
                    relative(pc, imm)
                }
            } else if w & 0x3b000000 == 0x18000000 {
                relative(pc, signed(((w >> 5) & 0x7ffff) as u64, 19) * 4)
            } else {
                None
            }
        }
        Arch::X64 => {
            let prefix = usize::from(
                b.first()
                    .map(|v| (0x40..=0x4f).contains(v))
                    .unwrap_or(false),
            );
            if b.len() >= prefix + 6
                && matches!(b[prefix], 0x8b | 0x8d | 0x89 | 0xff)
                && b[prefix + 1] & 0xc7 == 5
            {
                relative(
                    pc.checked_add(b.len() as u64)?,
                    i32::from_le_bytes(b[prefix + 2..prefix + 6].try_into().ok()?) as i64,
                )
            } else {
                None
            }
        }
        Arch::X86 => {
            if b.len() == 5 && matches!(b[0], 0xa1 | 0xa3) {
                Some(u32v(&b[1..], false) as u64)
            } else if b.len() >= 6 && matches!(b[0], 0x8b | 0x8d | 0x89 | 0xff) && b[1] & 0xc7 == 5
            {
                Some(u32v(&b[2..], false) as u64)
            } else {
                None
            }
        }
        Arch::Arm if b.len() == 4 => {
            let w = u32v(b, false);
            if w & 0x0f7f0000 == 0x051f0000 {
                relative(
                    pc.checked_add(8)?,
                    if w & 0x00800000 != 0 {
                        (w & 0xfff) as i64
                    } else {
                        -((w & 0xfff) as i64)
                    },
                )
            } else {
                None
            }
        }
        Arch::Thumb if b.len() == 2 => {
            let h = u16::from_le_bytes(b.try_into().ok()?);
            if h & 0xf800 == 0x4800 || h & 0xf800 == 0xa000 {
                (pc.checked_add(4)? & !3).checked_add((h & 0xff) as u64 * 4)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn prologue(a: Arch, b: &[u8], text: &str) -> bool {
    match a {
        Arch::X86 => b.starts_with(&[0x55, 0x89, 0xe5]) || b.starts_with(&[0x55, 0x8b, 0xec]),
        Arch::X64 => {
            b.starts_with(&[0x55, 0x48, 0x89, 0xe5]) || b.starts_with(&[0x55, 0x48, 0x8b, 0xec])
        }
        Arch::Thumb => {
            b.len() >= 2 && b[1] == 0xb5 || text.starts_with("push") && text.contains("lr")
        }
        Arch::Arm => b.len() >= 4 && u32v(b, false) & 0xffff4000 == 0xe92d4000,
        Arch::A64 => {
            text.starts_with("stp") && text.contains("x29, x30") && text.contains("sp")
                || text == "paciasp"
                || text == "pacibsp"
        }
    }
}
fn return_instruction(a: Arch, b: &[u8], text: &str) -> bool {
    match a {
        Arch::A64 => {
            text == "ret" || text.starts_with("ret ") || text == "retab" || text == "retaa"
        }
        Arch::X86 | Arch::X64 => matches!(b.first(), Some(0xc3 | 0xc2 | 0xcb | 0xca)),
        Arch::Arm | Arch::Thumb => {
            text == "bx lr"
                || text.starts_with("pop") && text.contains("pc")
                || text.starts_with("ldm") && text.contains("pc")
        }
    }
}

fn epilogue(a: Arch, bytes: &[u8], text: &str) -> bool {
    return_instruction(a, bytes, text)
        || match a {
            Arch::A64 => {
                (text.starts_with("ldp") && text.contains("x29, x30") && text.contains("sp"))
                    || text == "autiasp"
                    || text == "autibsp"
            }
            Arch::X86 | Arch::X64 => {
                text.starts_with("leave")
                    || (bytes.starts_with(&[0x5d]) && bytes.get(1) == Some(&0xc3))
            }
            _ => false,
        }
}

fn semantic_hint(a: Arch, bytes: &[u8], text: &str) -> Option<&'static str> {
    match a {
        Arch::X86 | Arch::X64 => {
            if text.starts_with("call") {
                Some("direct-call")
            } else if text.starts_with("j") || text.starts_with("loop") {
                Some("control-flow")
            } else {
                match bytes.first().copied()? {
                    0x50..=0x5f | 0x68 | 0x6a => Some("stack-operation"),
                    0x88..=0x8b | 0xb0..=0xbf | 0xc6 | 0xc7 => Some("data-movement"),
                    0x01 | 0x03 | 0x05 | 0x09 | 0x0b | 0x0d | 0x21 | 0x23 | 0x29
                    | 0x2b | 0x2d | 0x31 | 0x33 | 0x35 | 0x81 | 0x83 => Some("arithmetic-logic"),
                    0x39 | 0x3b | 0x3d | 0x85 => Some("compare-test"),
                    0xc3 | 0xc2 | 0xcb | 0xca => Some("return"),
                    _ => None,
                }
            }
        }
        Arch::Arm | Arch::Thumb => {
            if text.starts_with("bl") {
                Some("direct-call")
            } else if text.starts_with('b') || text.starts_with("bx") {
                Some("control-flow")
            } else if text.starts_with("ldr") || text.starts_with("str") || text.starts_with("mov") {
                Some("data-movement")
            } else if return_instruction(a, bytes, text) {
                Some("return")
            } else {
                None
            }
        }
        Arch::A64 => None,
    }
}

pub fn analyze(path: &str, digest: &str, size: &str, id: &str, o: &Options) -> io::Result<String> {
    let (mut file, before, _) = verified(path, digest, size, id)?;
    let image = load(&mut file, o)?;
    eprintln!(
        "VG_PROGRESS\t10\tmetadata\tWalking validated headers, sections and linkedit tables."
    );
    let symbols = symbols(&mut file, &image)?;
    let mut functions: BTreeMap<u64, (String, String)> = BTreeMap::new();
    for s in &symbols {
        if s.kind & 0xe == 0xe
            && s.section != 0
            && image.sections[s.section as usize - 1].executable
            && !s.name.is_empty()
        {
            let address = if image.cpu == 12 {
                s.address & !1
            } else {
                s.address
            };
            if !image.sections[s.section as usize - 1].contains(address) {
                return Err(bad("Defined symbol address outside its section"));
            }
            functions.entry(address).or_insert((
                s.name.clone(),
                format!(
                    "nlist section={}{}",
                    s.section,
                    if s.desc & 8 != 0 { " Thumb" } else { "" }
                ),
            ));
        }
    }
    for a in entry_points(&image)? {
        functions
            .entry(a)
            .or_insert((format!("entry_{a:x}"), "entry-point metadata".into()));
    }
    for a in function_starts(&mut file, &image)? {
        functions
            .entry(a)
            .or_insert((format!("sub_{a:x}"), "LC_FUNCTION_STARTS".into()));
    }
    let mut metadata = metadata(&image);
    for entry in entry_points(&image)? {
        push(
            &mut metadata,
            &format!("0x{entry:016x} entry-point metadata\n"),
        );
    }
    push(&mut metadata,"Instruction paging/CFG uses the selected primary executable section; other sections remain available as metadata, strings and table evidence.\n");
    let relocation_text = relocations(&mut file, &image, &symbols)?;
    let indirect = indirect_symbols(&mut file, &image, &symbols)?;
    let (export_text, export_names) = exports(&mut file, &image)?;
    for (a, name) in export_names {
        functions.entry(a).or_insert((name, "export-trie".into()));
    }
    let (strings, strings_limited) = strings(&mut file, &image)?;
    let (pointers, tables, tables_limited) = pointer_tables(&mut file, &image)?;
    let mut thumb_ranges = Vec::<(u64, u64)>::new();
    if image.arch == Arch::Arm {
        for symbol in symbols.iter().filter(|s| s.desc & 8 != 0 && s.section != 0) {
            let section = &image.sections[symbol.section as usize - 1];
            let start = symbol.address & !1;
            if !section.executable || !section.contains(start) {
                continue;
            }
            let end = functions
                .keys()
                .copied()
                .filter(|address| *address > start && section.contains(*address))
                .min()
                .unwrap_or(section.address + section.size);
            if end > start {
                thumb_ranges.push((start, end));
            }
        }
        thumb_ranges.sort_unstable();
        thumb_ranges.dedup();
    }
    if !thumb_ranges.is_empty() {
        push(
            &mut metadata,
            &format!(
                "Mixed ARM/Thumb decode: {} metadata-marked Thumb range(s) selected; register-indirect mode changes remain unresolved.\n",
                thumb_ranges.len()
            ),
        );
    }
    let no_code = Section::default();
    let s = image
        .code
        .map(|index| &image.sections[index])
        .unwrap_or(&no_code);
    let mut instructions = Vec::new();
    let mut candidates = String::new();
    let mut epilogues = String::new();
    let mut annotated = String::new();
    let mut direct = String::new();
    let mut calls = String::new();
    let mut leaders = BTreeSet::new();
    let mut truncated = strings_limited || tables_limited;
    let mut decode_count = 0;
    let mut scan_bytes = 0;
    let ready = code_ready(&image);
    let state = match &ready {
        Ok(_) => "",
        Err(e) => {
            push(
                &mut candidates,
                &format!("Instruction analysis unavailable: {e}\n"),
            );
            "metadata-only"
        }
    };
    if ready.is_ok() {
        let arm_decoder = Decoder::new(Arch::Arm)?;
        let thumb_decoder = Decoder::new(Arch::Thumb)?;
        let primary_decoder = Decoder::new(image.arch)?;
        let ranges = data_ranges(&mut file, &image)?;
        let n = s.size.min(SCAN as u64) as usize;
        scan_bytes = n;
        truncated |= (n as u64) < s.size;
        let code = read(&mut file, s.offset, n)?;
        eprintln!(
            "VG_PROGRESS\t35\tdecode\tDecoding instructions and collecting structural candidates."
        );
        let mut at = 0;
        leaders.insert(s.address);
        functions.entry(s.address).or_insert((
            format!("sub_{:x}", s.address),
            "section-start candidate".into(),
        ));
        while at < code.len() {
            if instructions.len() >= 262_144 {
                truncated = true;
                break;
            }
            let pc = s.address + at as u64;
            let pos = s.offset + at as u64;
            let mode = if image.arch == Arch::Arm
                && thumb_ranges.iter().any(|(lo, hi)| pc >= *lo && pc < *hi)
            {
                Arch::Thumb
            } else {
                image.arch
            };
            let decoder = if image.arch == Arch::Arm && mode == Arch::Thumb {
                &thumb_decoder
            } else if image.arch == Arch::Arm {
                &arm_decoder
            } else {
                &primary_decoder
            };
            let unit = mode.unit();
            let in_data = ranges
                .iter()
                .any(|&(lo, hi)| pos < hi && pos + unit > lo);
            let (n, text, known) = if in_data {
                (
                    (unit as usize).min(code.len() - at),
                    ".data-in-code".into(),
                    false,
                )
            } else {
                decoder.instruction(&code[at..], pc, mode)
            };
            let known = known
                && !ranges
                    .iter()
                    .any(|&(lo, hi)| pos < hi && pos + n as u64 > lo);
            if known {
                decode_count += 1;
                if prologue(mode, &code[at..], &text) {
                    functions
                        .entry(pc)
                        .or_insert((format!("sub_{pc:x}"), "prologue candidate".into()));
                    push(
                        &mut candidates,
                        &format!("0x{pc:016x} prologue candidate: {text}\n"),
                    );
                }
            }
            let known = known
                && !ranges
                    .iter()
                    .any(|&(lo, hi)| pos < hi && pos + n as u64 > lo);
            let text = if !known
                && ranges
                    .iter()
                    .any(|&(lo, hi)| pos < hi && pos + n as u64 > lo)
            {
                ".data-in-code".to_string()
            } else {
                text
            };
            let bytes = code[at..at + n].to_vec();
            let transfer = if known {
                branch(mode, &bytes, pc)
            } else {
                None
            };
            let ret = known && return_instruction(mode, &bytes, &text);
            let op = text.split_whitespace().next().unwrap_or("");
            let unknown_transfer = transfer.is_none()
                && (matches!(
                    op,
                    "jmp" | "jmpq" | "jmpl" | "br" | "bx" | "bxj" | "b" | "b.w"
                ) || [
                    "beq", "bne", "bcs", "bhs", "bcc", "blo", "bmi", "bpl", "bvs", "bvc", "bhi",
                    "bls", "bge", "blt", "bgt", "ble",
                ]
                .iter()
                .any(|prefix| op.starts_with(prefix)));
            let terminal = ret
                || unknown_transfer
                || transfer.map(|b| b.1 == "branch").unwrap_or(false)
                || text.starts_with("jmp ")
                || text.starts_with("br ")
                || text.starts_with("bx ")
                || !known;
            if known && epilogue(image.arch, &code[at..], &text) {
                push(
                    &mut epilogues,
                    &format!("0x{pc:016x} return/epilogue candidate: {text}\n"),
                );
            }
            if let Some((target, kind)) = transfer {
                push(
                    &mut direct,
                    &format!("0x{pc:016x} --{kind}--> 0x{target:016x}\n"),
                );
                if s.contains(target) {
                    leaders.insert(target);
                    if kind == "call" {
                        functions
                            .entry(target)
                            .or_insert((format!("sub_{target:x}"), "direct-call candidate".into()));
                    }
                }
                if kind == "call" {
                    let callee = symbols
                        .iter()
                        .find(|s| s.address == target)
                        .map(|s| s.name.as_str())
                        .unwrap_or("<unnamed>");
                    push(
                        &mut calls,
                        &format!("0x{pc:016x} call target=0x{target:016x} symbol={callee}\n"),
                    );
                }
                if pc + (n as u64) < s.address + scan_bytes as u64 {
                    leaders.insert(pc + n as u64);
                }
            }
            if terminal && pc + (n as u64) < s.address + scan_bytes as u64 {
                leaders.insert(pc + n as u64);
            }
            instructions.push(Instruction {
                address: pc,
                offset: pos,
                bytes,
                text,
                known,
                branch: transfer,
                terminal,
            });
            at += n;
        }
        scan_bytes = at;
    }
    let boundaries: BTreeSet<u64> = instructions
        .iter()
        .filter(|v| v.known)
        .map(|v| v.address)
        .collect();
    let scan_end = instructions
        .last()
        .map(|v| v.address + v.bytes.len() as u64)
        .unwrap_or(s.address);
    // Keep metadata-backed functions outside the scanned section; candidates must
    // land on a known instruction boundary in the selected decode mode.
    functions.retain(|a, v| {
        v.1.starts_with("nlist")
            || v.1 == "LC_FUNCTION_STARTS"
            || v.1 == "export-trie"
            || v.1 == "entry-point metadata"
            || boundaries.contains(a)
    });
    leaders.extend(functions.keys().filter(|a| boundaries.contains(a)).copied());
    leaders.retain(|a| boundaries.contains(a));
    let mut code_references = String::new();
    let mut reference_budget = SCAN;
    for instruction in instructions.iter().filter(|v| v.known) {
        if code_references.len() > TEXT - 1024 {
            break;
        }
        if let Some(target) = data_reference(image.arch, &instruction.bytes, instruction.address) {
            if let Some(section) = image.sections.iter().find(|s| s.contains(target)) {
                push(
                    &mut code_references,
                    &format!(
                        "0x{:016x} reference -> 0x{target:016x} {}.{}\n",
                        instruction.address, section.segment, section.name
                    ),
                );
                if !section.zero && !section.executable && reference_budget > 0 {
                    let count = (section.size - (target - section.address))
                        .min(192)
                        .min(reference_budget as u64) as usize;
                    let bytes = read(&mut file, section.offset + target - section.address, count)?;
                    reference_budget -= count;
                    let length = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
                    if length >= 4
                        && bytes[..length]
                            .iter()
                            .all(|b| b.is_ascii_graphic() || *b == b' ')
                    {
                        push(
                            &mut code_references,
                            &format!(
                                "0x{:016x} string-reference -> 0x{target:016x} {:?}\n",
                                instruction.address,
                                name(&bytes[..length])
                            ),
                        );
                    }
                }
            }
        }
    }
    push(&mut code_references, &pointers);
    let mut function_text = String::new();
    for (&a, (name, source)) in &functions {
        push(
            &mut function_text,
            &format!("0x{a:016x} name={name} source={source}\n"),
        );
    }
    let mut cfg = String::new();
    let mut blocks: Vec<Vec<&Instruction>> = Vec::new();
    let mut current = Vec::new();
    for instruction in &instructions {
        if !instruction.known {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
            continue;
        }
        if leaders.contains(&instruction.address) && !current.is_empty() {
            blocks.push(std::mem::take(&mut current));
        }
        current.push(instruction);
        if instruction.terminal {
            blocks.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    let block_count = blocks.len();
    for (index, block) in blocks.iter().enumerate() {
        if cfg.len() > TEXT - 2048 {
            break;
        }
        let first = block[0];
        let last = block[block.len() - 1];
        let end = last.address + last.bytes.len() as u64;
        push(
            &mut cfg,
            &format!("block B{index:04} 0x{:016x}..0x{end:016x}\n", first.address),
        );
        for instruction in block {
            if let Some((target, kind)) = instruction.branch {
                push(
                    &mut cfg,
                    &format!(
                        "    {kind} 0x{:016x} -> 0x{target:016x}\n",
                        instruction.address
                    ),
                );
            }
        }
        if !last.terminal && boundaries.contains(&end) {
            push(
                &mut cfg,
                &format!("    fallthrough 0x{:016x} -> 0x{end:016x}\n", last.address),
            );
        }
    }
    for v in &instructions {
        let label = functions
            .get(&v.address)
            .map(|v| format!(" ; name={} source={}", v.0, v.1))
            .unwrap_or_default();
        let semantic = semantic_hint(image.arch, &v.bytes, &v.text)
            .map(|hint| format!(" ; semantic={hint}"))
            .unwrap_or_default();
        push(
            &mut annotated,
            &format!(
                "0x{:016x} 0x{:016x} {:<30} {}{}{}\n",
                v.address,
                v.offset,
                v.bytes
                    .iter()
                    .map(|v| format!("{v:02x}"))
                    .collect::<String>(),
                v.text,
                label,
                semantic
            ),
        );
    }
    let mut intelligence = candidates;
    push(&mut intelligence, &epilogues);
    push(&mut intelligence,"Prologues, return instructions, direct-call targets and pointer tables are candidates, not proof of function extent. Mixed ARM/Thumb mode requires a separate import with the correct mode.\n");
    let symbol_text = symbols
        .iter()
        .filter(|s| !s.name.is_empty())
        .take(2000)
        .map(|s| {
            format!(
                "0x{:016x} name={} n_type={:#x} section={} n_desc={:#x}\n",
                s.address, s.name, s.kind, s.section, s.desc
            )
        })
        .collect::<String>();
    let mut symbol_text = symbol_text;
    symbol_text.truncate(symbol_text.len().min(TEXT));
    let imports = symbols
        .iter()
        .filter(|s| s.kind & 0xe == 0 && !s.name.is_empty())
        .take(2000)
        .map(|s| format!("name={} undefined nlist ordinal={}\n", s.name, s.desc >> 8))
        .collect::<String>();
    let mut imports = imports;
    imports.truncate(imports.len().min(TEXT));
    let quality=format!("Selected decode: {} {state}\nCode bytes scanned: {scan_bytes} / {}\nDecoded instructions: {decode_count} / {}\nScan end: {scan_end:#x}\nCode scan budget: {SCAN} bytes / 262144 instructions. Data strings/tables each have a {SCAN}-byte budget. Text views are capped at {TEXT} bytes; counts can exceed displayed rows. Unknown instructions/data-in-code terminate inferred flow. No CFG edge is inferred from a pointer-table candidate.\n",image.arch.name(),s.size,instructions.len());
    let summary=format!("{} structural analysis\nSelected image offset: {:#x}\nCode: {:#x}..{:#x}\nFunctions/named or candidate starts: {}\nNlist records: {}\nBasic blocks: {block_count}\nInstructions scanned: {}\n{}\nAdvanced ARM64 SSA/runtime reconstruction remains available through the original ARM64 analysis path. Multi-architecture structural analysis does not claim semantic lifting or decompilation.\n",image.arch.name(),image.origin,s.address,s.address+s.size,functions.len(),symbols.len(),instructions.len(),quality);
    let mut values: BTreeMap<&str, String> = BTreeMap::new();
    for key in REPORT_TEXT_FIELDS {
        values.insert(
            key,
            "No applicable records decoded by this structural backend.".into(),
        );
    }
    for (key,value) in [("functions",function_text),("metadata",metadata),("strings",strings),("relocations",relocation_text),("indirect_symbols",indirect),("export_trie",export_text),("pointer_slots",pointers.clone()),("data_references",code_references),("jump_tables",tables),("xrefs",direct),("call_graph",calls),("cfg",cfg),("annotated_disassembly",annotated),("function_intelligence",intelligence),("symbolication",symbol_text.clone()),("symbol_provenance",symbol_text),("dyld_imports",imports),("analysis_quality",quality),("analysis_insights","Review metadata-backed names first, then prologue/call/table candidates. Unresolved indirect transfers and mixed-mode firmware require analyst review.".into()),("summary",summary)]{values.insert(key,value);}
    for key in [
        "ir",
        "ssa",
        "dataflow",
        "dominators",
        "phi_nodes",
        "dominance_frontiers",
        "ssa_renamed",
        "phi_operands",
        "interblock_values",
        "predecessor_values",
        "propagated_values",
    ] {
        values.insert(key,"Semantic lifting/SSA is unavailable for this structural backend; no register values or decompiler results are invented.".into());
    }
    truncated |= values.values().any(|v| v.len() >= TEXT - 4096);
    if fingerprint(&before) != fingerprint(&file.metadata()?) {
        return Err(bad("Captured object changed during analysis"));
    }
    eprintln!(
        "VG_PROGRESS\t95\tfinalize\tRechecking source identity and formatting bounded evidence."
    );
    let mut count = 0;
    for (source, key, kind) in [
        ("Functions", "functions", "Function"),
        ("Strings", "strings", "Data"),
        ("Function Intelligence", "function_intelligence", "Function"),
        ("Metadata", "metadata", "Metadata"),
        ("Xrefs", "xrefs", "Control Flow"),
        ("Jump Tables", "jump_tables", "Control Flow"),
    ] {
        for (ordinal, line) in values[key].lines().take(2000).enumerate() {
            if line.len() > 4096 {
                continue;
            }
            let address = line
                .split_whitespace()
                .next()
                .filter(|v| v.starts_with("0x") && v[2..].chars().all(|c| c.is_ascii_hexdigit()))
                .map(super::quote)
                .unwrap_or_else(|| "null".into());
            eprintln!("VG_EVIDENCE\t{{\"source\":{},\"workspace\":{},\"kind\":{},\"text\":{},\"address\":{},\"symbol\":null,\"ordinal\":{},\"focus_priority\":70}}",super::quote(source),super::quote(source),super::quote(kind),super::quote(line),address,ordinal);
            count += 1;
        }
    }
    eprintln!("VG_EVIDENCE_MANIFEST\t{{\"schema\":1,\"record_count\":{count},\"truncated\":{truncated},\"stage_fingerprints\":{{}}}}");
    let mut out=format!("{{\"protocol\":1,\"kind\":\"analysis_result\",\"request_id\":{},\"source_sha256\":{},\"function_count\":{},\"symbol_count\":{},\"basic_block_count\":{},\"analyzed_instructions\":{},\"truncated\":{}",super::quote(id),super::quote(digest),functions.len(),symbols.len(),block_count,instructions.len(),truncated);
    for (key, value) in values {
        out.push_str(&format!(",{}:{}", super::quote(key), super::quote(&value)));
    }
    out.push('}');
    Ok(out)
}

const REPORT_TEXT_FIELDS: &[&str] = &[
    "functions",
    "cfg",
    "ir",
    "strings",
    "xrefs",
    "call_graph",
    "dataflow",
    "ssa",
    "dominators",
    "phi_nodes",
    "dominance_frontiers",
    "relocations",
    "jump_tables",
    "metadata",
    "runtime_records",
    "annotated_disassembly",
    "ssa_renamed",
    "runtime_graph",
    "swift_types",
    "selector_xrefs",
    "phi_operands",
    "objc_classes",
    "swift_contexts",
    "call_values",
    "chained_fixups",
    "chained_starts",
    "objc_protocols",
    "swift_fields",
    "swift_field_records",
    "interblock_values",
    "predecessor_values",
    "chained_pointers",
    "chained_multistarts",
    "arm64e_pointers",
    "swift_field_ownership",
    "propagated_values",
    "indirect_calls",
    "dyld_imports",
    "dyld_linking",
    "softlinking",
    "symbolication",
    "symbol_resolver",
    "apple_symbolication",
    "arm64e_bind_symbols",
    "dylib_ordinals",
    "resolved_imports",
    "authenticated_bind_targets",
    "indirect_symbols",
    "pointer_slots",
    "export_trie",
    "symbol_provenance",
    "linkage_records",
    "symbol_callers",
    "got_auth_slots",
    "import_export_collisions",
    "function_intelligence",
    "data_references",
    "tail_call_candidates",
    "function_neighborhoods",
    "analysis_quality",
    "analysis_insights",
    "summary",
];

pub fn legacy_arm64(path: &str, o: &Options) -> bool {
    if o.offset != 0
        || o.base != 0
        || o.format == "raw"
        || !matches!(o.architecture.as_str(), "auto" | "aarch64")
    {
        return false;
    }
    let mut head = [0; 32];
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    if file.read_exact(&mut head).is_err() {
        return false;
    }
    magic(&head) == Some((64, false))
        && u32v(&head[4..], false) == 0x0100000c
        && matches!(u32v(&head[12..], false), 2 | 6 | 8)
        && file
            .metadata()
            .ok()
            .and_then(|m| thin(&mut file, 0, m.len(), o).ok())
            .map(|i| i.code.is_some())
            .unwrap_or(false)
}
pub fn arm64_patterns(
    code: &[u8],
    address: u64,
    offset: u64,
    ranges: &[(u64, u64, u16)],
) -> String {
    let mut out = String::new();
    let decoder = match Decoder::new(Arch::A64) {
        Ok(d) => d,
        Err(_) => return out,
    };
    for (index, b) in code.chunks_exact(4).enumerate() {
        let pc = address + index as u64 * 4;
        let position = offset + index as u64 * 4;
        if ranges.iter().any(|r| position < r.1 && position + 4 > r.0) {
            continue;
        }
        let (_, text, known) = decoder.instruction(b, pc, Arch::A64);
        if known && prologue(Arch::A64, b, &text) {
            push(
                &mut out,
                &format!("0x{pc:016x} prologue candidate: {text}\n"),
            );
        }
        if known && epilogue(Arch::A64, b, &text) {
            push(
                &mut out,
                &format!("0x{pc:016x} return/epilogue candidate: {text}\n"),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checked_branch_addresses() {
        assert_eq!(
            branch(Arch::X64, &[0xe8, 1, 0, 0, 0], 0x1000),
            Some((0x1006, "call"))
        );
        assert_eq!(
            branch(Arch::X86, &[0xeb, 0xfe], 0x1000),
            Some((0x1000, "branch"))
        );
        assert_eq!(
            branch(Arch::A64, &0x17ff_ffffu32.to_le_bytes(), 0x1000),
            Some((0xffc, "branch"))
        );
        assert_eq!(
            branch(Arch::Thumb, &[0x00, 0xf0, 0x02, 0xf8], 0x8c0e),
            Some((0x8c16, "call"))
        );
        assert_eq!(
            branch(Arch::Arm, &0xeb00_0000u32.to_le_bytes(), 0x1000),
            Some((0x1008, "call"))
        );
        assert_eq!(branch(Arch::X64, &[0xeb, 0x7f], u64::MAX - 2), None);
        assert_eq!(relative(0, -1), None);
    }
    #[test]
    fn import_settings_checked() {
        let values = ["sepos", "thumb", "0x1000", "0x94000"].map(String::from);
        let o = Options::parse(&values).unwrap();
        assert_eq!(o.base, 4096);
        assert_eq!(o.offset, 606208);
        assert!(Options::parse(&["macho", "unknown", "0", "0"].map(String::from)).is_err());
        assert!(
            Options::parse(&["raw", "aarch64", "18446744073709551616", "0"].map(String::from))
                .is_err()
        );
        assert!(range(u64::MAX, 2, u64::MAX).is_err());
    }
    #[test]
    fn malformed_uleb_rejected() {
        assert!(uleb(&[0x80], &mut 0).is_err());
        assert!(uleb(&[0xff; 10], &mut 0).is_err());
        assert_eq!(uleb(&[0x81, 0x01], &mut 0).unwrap(), 129);
    }
    #[test]
    fn architecture_patterns_are_candidates() {
        assert!(prologue(Arch::X64, &[0x55, 0x48, 0x89, 0xe5], "pushq %rbp"));
        assert!(!prologue(
            Arch::X64,
            &[0x55, 0x48, 0x89, 0xe4],
            "pushq %rbp"
        ));
        assert!(epilogue(Arch::A64, &[], "ldp x29, x30, [sp], #0x10"));
        assert!(return_instruction(
            Arch::Thumb,
            &[0x80, 0xbd],
            "pop {r7, pc}"
        ));
    }
}
