//! Bounded Apple firmware payload decoding.
//!
//! The container is never treated as code.  We only publish a decoded payload
//! when a decoder returns bytes containing a validated Mach-O header.  On
//! macOS the system Compression framework supplies the Apple codecs; the
//! small `complzss` path covers the legacy kernelcache/IMG3 LZSS wrapper.

const MAX_OUTPUT: usize = 512 * 1024 * 1024;
const MAX_ZIP_ENTRIES: usize = 256;
const MAX_ZIP_NAME: usize = 256;

#[cfg(target_os = "macos")]
extern "C" {
    fn dlopen(path: *const std::os::raw::c_char, mode: i32) -> *mut std::os::raw::c_void;
    fn dlsym(
        handle: *mut std::os::raw::c_void,
        name: *const std::os::raw::c_char,
    ) -> *mut std::os::raw::c_void;
}

#[cfg(target_os = "macos")]
fn system_decode(input: &[u8], algorithm: u32) -> Option<Vec<u8>> {
    type Decode = unsafe extern "C" fn(*mut u8, usize, *const u8, usize, *mut u8, u32) -> usize;
    let library =
        std::ffi::CString::new("/System/Library/Frameworks/Compression.framework/Compression")
            .ok()?;
    let symbol = std::ffi::CString::new("compression_decode_buffer").ok()?;
    let handle = unsafe { dlopen(library.as_ptr(), 1) };
    if handle.is_null() {
        return None;
    }
    let function = unsafe { dlsym(handle, symbol.as_ptr()) };
    if function.is_null() {
        return None;
    }
    let decode: Decode = unsafe { std::mem::transmute(function) };
    let mut capacity = input.len().saturating_mul(4).max(4096);
    while capacity <= MAX_OUTPUT {
        let mut output = vec![0; capacity];
        let size = unsafe {
            decode(
                output.as_mut_ptr(),
                output.len(),
                input.as_ptr(),
                input.len(),
                std::ptr::null_mut(),
                algorithm,
            )
        };
        if size != 0 {
            output.truncate(size);
            return Some(output);
        }
        capacity = capacity.saturating_mul(2);
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn system_decode(_input: &[u8], _algorithm: u32) -> Option<Vec<u8>> {
    None
}

fn macho_offset(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).take(4 * 1024 * 1024).position(|w| {
        matches!(
            w,
            [0xce, 0xfa, 0xed, 0xfe]
                | [0xfe, 0xed, 0xfa, 0xce]
                | [0xcf, 0xfa, 0xed, 0xfe]
                | [0xfe, 0xed, 0xfa, 0xcf]
                | [0xca, 0xfe, 0xba, 0xbe]
                | [0xbe, 0xba, 0xfe, 0xca]
        )
    })
}

fn lzss(input: &[u8]) -> Option<Vec<u8>> {
    // complzss payloads use the classic 4 KiB window / 3..18 match form.
    // Try both flag-bit orders used by Apple-era tools and reject malformed
    // streams rather than returning a plausible partial image.
    for msb_first in [false, true] {
        let mut out = Vec::new();
        let mut at = 0;
        let mut flags = 0u16;
        let mut bits = 0;
        let mut ok = true;
        while at < input.len() && out.len() < MAX_OUTPUT {
            if bits == 0 {
                if at + 2 > input.len() {
                    ok = false;
                    break;
                }
                flags = u16::from_le_bytes([input[at], input[at + 1]]);
                at += 2;
                bits = 16;
            }
            let literal = if msb_first {
                flags & 0x8000 != 0
            } else {
                flags & 1 != 0
            };
            if msb_first {
                flags <<= 1;
            } else {
                flags >>= 1;
            }
            bits -= 1;
            if literal {
                if at >= input.len() {
                    ok = false;
                    break;
                }
                out.push(input[at]);
                at += 1;
            } else {
                if at + 2 > input.len() {
                    ok = false;
                    break;
                }
                let pair = u16::from_le_bytes([input[at], input[at + 1]]);
                at += 2;
                let distance = (pair & 0x0fff) as usize;
                let length = ((pair >> 12) as usize) + 3;
                if distance == 0
                    || distance > out.len()
                    || out.len().saturating_add(length) > MAX_OUTPUT
                {
                    ok = false;
                    break;
                }
                for _ in 0..length {
                    let index = out.len() - distance;
                    let byte = out[index];
                    out.push(byte);
                }
            }
        }
        if ok && !out.is_empty() {
            if let Some(offset) = macho_offset(&out) {
                return Some(out[offset..].to_vec());
            }
        }
    }
    None
}

fn zip_entries(input: &[u8]) -> Vec<(u16, &[u8])> {
    let mut entries = Vec::new();
    let mut at = 0usize;
    while at + 30 <= input.len() && entries.len() < MAX_ZIP_ENTRIES {
        if &input[at..at + 4] != b"PK\x03\x04" {
            break;
        }
        let flags = u16::from_le_bytes([input[at + 6], input[at + 7]]);
        let method = u16::from_le_bytes([input[at + 8], input[at + 9]]);
        let compressed = u32::from_le_bytes(input[at + 18..at + 22].try_into().unwrap()) as usize;
        let name_len = u16::from_le_bytes([input[at + 26], input[at + 27]]) as usize;
        let extra_len = u16::from_le_bytes([input[at + 28], input[at + 29]]) as usize;
        if name_len == 0 || name_len > MAX_ZIP_NAME || flags & 0x08 != 0 {
            break;
        }
        let start = at.saturating_add(30 + name_len + extra_len);
        let end = start.saturating_add(compressed);
        if end > input.len() {
            break;
        }
        if matches!(method, 0 | 8) {
            entries.push((method, &input[start..end]));
        }
        at = end;
    }
    entries
}

fn deflate_decode(input: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut decoder = flate2::read::DeflateDecoder::new(input);
    let mut output = Vec::new();
    let mut limited = (&mut decoder).take((MAX_OUTPUT + 1) as u64);
    limited.read_to_end(&mut output).ok()?;
    (output.len() <= MAX_OUTPUT).then_some(output)
}

fn zip_payloads(input: &[u8]) -> Vec<(u16, Vec<u8>)> {
    let mut payloads = zip_entries(input)
        .into_iter()
        .filter_map(|(method, bytes)| {
            let payload = match method {
                0 => Some(bytes.to_vec()),
                8 => deflate_decode(bytes),
                _ => None,
            }?;
            Some((method, payload))
        })
        .collect::<Vec<_>>();
    if !payloads.is_empty() {
        return payloads;
    }
    // Data-descriptor archives leave sizes in the local header as zero.  The
    // central directory is authoritative for the extent, so use it when the
    // local-header walk cannot safely advance.
    let Some(eocd) = input.windows(4).rposition(|window| window == b"PK\x05\x06") else {
        return payloads;
    };
    if eocd + 22 > input.len() {
        return payloads;
    }
    let directory_size = u32::from_le_bytes(input[eocd + 12..eocd + 16].try_into().unwrap()) as usize;
    let directory_offset = u32::from_le_bytes(input[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
    let directory_end = directory_offset.saturating_add(directory_size);
    if directory_end > input.len() {
        return payloads;
    }
    let mut at = directory_offset;
    while at + 46 <= directory_end && payloads.len() < MAX_ZIP_ENTRIES {
        if &input[at..at + 4] != b"PK\x01\x02" {
            break;
        }
        let flags = u16::from_le_bytes([input[at + 8], input[at + 9]]);
        let method = u16::from_le_bytes([input[at + 10], input[at + 11]]);
        let compressed = u32::from_le_bytes(input[at + 20..at + 24].try_into().unwrap()) as usize;
        let name_len = u16::from_le_bytes([input[at + 28], input[at + 29]]) as usize;
        let extra_len = u16::from_le_bytes([input[at + 30], input[at + 31]]) as usize;
        let comment_len = u16::from_le_bytes([input[at + 32], input[at + 33]]) as usize;
        let local_offset = u32::from_le_bytes(input[at + 42..at + 46].try_into().unwrap()) as usize;
        let next = at.saturating_add(46 + name_len + extra_len + comment_len);
        if next > directory_end || name_len == 0 || name_len > MAX_ZIP_NAME || flags & 1 != 0 {
            break;
        }
        if local_offset + 30 > input.len() || &input[local_offset..local_offset + 4] != b"PK\x03\x04" {
            break;
        }
        let local_name = u16::from_le_bytes([input[local_offset + 26], input[local_offset + 27]]) as usize;
        let local_extra = u16::from_le_bytes([input[local_offset + 28], input[local_offset + 29]]) as usize;
        let data_start = local_offset.saturating_add(30 + local_name + local_extra);
        let data_end = data_start.saturating_add(compressed);
        if data_end > input.len() || !matches!(method, 0 | 8) {
            at = next;
            continue;
        }
        let data = &input[data_start..data_end];
        let decoded = if method == 0 { Some(data.to_vec()) } else { deflate_decode(data) };
        if let Some(decoded) = decoded {
            payloads.push((method, decoded));
        }
        at = next;
    }
    payloads
}

pub fn zip_inventory(input: &[u8]) -> String {
    let mut out = String::new();
    let mut at = 0usize;
    let mut count = 0usize;
    while at + 30 <= input.len() && count < MAX_ZIP_ENTRIES {
        if &input[at..at + 4] != b"PK\x03\x04" {
            break;
        }
        let flags = u16::from_le_bytes([input[at + 6], input[at + 7]]);
        let method = u16::from_le_bytes([input[at + 8], input[at + 9]]);
        let compressed = u32::from_le_bytes(input[at + 18..at + 22].try_into().unwrap());
        let uncompressed = u32::from_le_bytes(input[at + 22..at + 26].try_into().unwrap());
        let name_len = u16::from_le_bytes([input[at + 26], input[at + 27]]) as usize;
        let extra_len = u16::from_le_bytes([input[at + 28], input[at + 29]]) as usize;
        if name_len == 0 || name_len > MAX_ZIP_NAME || flags & 0x08 != 0 {
            break;
        }
        let name_start = at + 30;
        let name_end = name_start + name_len;
        let data_start = name_end + extra_len;
        let end = data_start.saturating_add(compressed as usize);
        if end > input.len() {
            break;
        }
        let name = String::from_utf8_lossy(&input[name_start..name_end]);
        out.push_str(&format!(
            "entry={name:?} method={method} compressed={compressed:#x} uncompressed={uncompressed:#x} stored={}\n",
            method == 0
        ));
        count += 1;
        at = end;
    }
    if count == MAX_ZIP_ENTRIES {
        out.push_str("ZIP entry inventory capped at 256 entries.\n");
    }
    out
}

/// Decode a bounded Apple payload and return the first result that contains a
/// Mach-O header. The returned scheme is evidence, not an assertion that an
/// encrypted or signed container was validly authenticated.
fn decode_payload_inner(input: &[u8], depth: usize) -> Option<(&'static str, Vec<u8>)> {
    if input.len() > 8 * 1024 * 1024 * 1024 {
        return None;
    }
    let mut bodies: Vec<&[u8]> = vec![input];
    if input.len() > 4 {
        bodies.push(&input[4..]);
    }
    if input.len() > 16 {
        bodies.push(&input[16..]);
    }
    if input.len() > 32 {
        bodies.push(&input[32..]);
    }
    for marker in [b"IM4P".as_slice(), b"bvx2", b"bvxn", b"complzss"] {
        if let Some(offset) = input.windows(marker.len()).position(|window| window == marker) {
            if input.len() > offset + marker.len() {
                bodies.push(&input[offset + marker.len()..]);
            }
        }
    }
    for (_, entry) in zip_entries(input) {
        bodies.push(entry);
    }

    if let Some(offset) = input.windows(4).position(|window| window == b"bvxn") {
        if let Ok(decoded) = lzvn::decode_bvxn(&input[offset..]) {
            if decoded.len() <= MAX_OUTPUT {
                if let Some(macho) = macho_offset(&decoded) {
                    return Some(("bvxn-LZVN", decoded[macho..].to_vec()));
                }
            }
        }
    }

    let algorithms = [
        ("LZFSE", 0x801),
        ("LZ4", 0x100),
        ("LZ4-raw", 0x101),
        ("LZMA", 0x306),
        ("zlib", 0x205),
    ];
    for body in bodies {
        for (name, algorithm) in algorithms {
            if let Some(decoded) = system_decode(body, algorithm) {
                if let Some(offset) = macho_offset(&decoded) {
                    return Some((name, decoded[offset..].to_vec()));
                }
            }
        }
    }
    let lzss_input = input.strip_prefix(b"complzss").unwrap_or(input);
    if let Some(decoded) = lzss(lzss_input) {
        return Some(("complzss-LZSS", decoded));
    }
    if depth < 2 {
        for (method, payload) in zip_payloads(input) {
            if let Some(offset) = macho_offset(&payload) {
                return Some((if method == 0 { "IPSW-stored" } else { "IPSW-deflate" }, payload[offset..].to_vec()));
            }
            if let Some((scheme, decoded)) = decode_payload_inner(&payload, depth + 1) {
                return Some((scheme, decoded));
            }
        }
    }
    None
}

pub fn decode_payload(input: &[u8]) -> Option<(&'static str, Vec<u8>)> {
    decode_payload_inner(input, 0)
}

pub fn marker_name(input: &[u8]) -> &'static str {
    if input.starts_with(b"bvx2") {
        "bvx2/LZFSE"
    } else if input.starts_with(b"bvxn") {
        "bvxn/LZVN"
    } else if input.starts_with(b"bvx-") {
        "bvx-/stored"
    } else if input.starts_with(b"complzss") {
        "complzss/LZSS"
    } else if input.starts_with(b"IMG3") {
        "IMG3"
    } else if input.starts_with(b"IMG4") {
        "IMG4"
    } else if input.starts_with(b"PK\x03\x04") {
        "IPSW/ZIP"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_inventory_is_explicit() {
        assert_eq!(marker_name(b"bvx2xxxx"), "bvx2/LZFSE");
        assert_eq!(marker_name(b"complzss"), "complzss/LZSS");
        assert_eq!(marker_name(b"IMG4"), "IMG4");
    }

    #[test]
    fn stored_zip_entries_are_bounded_and_visible() {
        let mut zip = Vec::new();
        zip.extend_from_slice(b"PK\x03\x04");
        zip.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        zip.extend_from_slice(&(4u32).to_le_bytes());
        zip.extend_from_slice(&(4u32).to_le_bytes());
        zip.extend_from_slice(&(5u16).to_le_bytes());
        zip.extend_from_slice(&(0u16).to_le_bytes());
        zip.extend_from_slice(b"x.binTEST");
        assert_eq!(zip_entries(&zip), vec![(0, b"TEST".as_slice())]);
        assert!(zip_inventory(&zip).contains("entry=\"x.bin\""));
    }

    #[test]
    fn raw_deflate_round_trips_with_output_budget() {
        use std::io::Write;
        let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(b"Apple firmware payload").unwrap();
        let encoded = encoder.finish().unwrap();
        assert_eq!(deflate_decode(&encoded).as_deref(), Some(b"Apple firmware payload".as_slice()));
    }

    #[test]
    fn deflated_zip_entries_are_extracted() {
        use std::io::Write;
        let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(b"firmware Mach-O candidate").unwrap();
        let encoded = encoder.finish().unwrap();
        let name = b"kernelcache";
        let mut zip = Vec::new();
        zip.extend_from_slice(b"PK\x03\x04");
        zip.extend_from_slice(&[20, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        zip.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
        zip.extend_from_slice(&(25u32).to_le_bytes());
        zip.extend_from_slice(&(name.len() as u16).to_le_bytes());
        zip.extend_from_slice(&(0u16).to_le_bytes());
        zip.extend_from_slice(name);
        zip.extend_from_slice(&encoded);
        assert_eq!(zip_entries(&zip).len(), 1);
        assert_eq!(zip_payloads(&zip)[0].1, b"firmware Mach-O candidate");
        assert!(zip_inventory(&zip).contains("method=8"));
    }

    #[test]
    fn bvxn_lzvn_decodes_a_macho_candidate() {
        let mut plaintext = vec![0xcf, 0xfa, 0xed, 0xfe];
        plaintext.extend_from_slice(b"kernel");
        let encoded = lzvn::encode_bvxn(&plaintext).unwrap();
        let (scheme, decoded) = decode_payload(&encoded).unwrap();
        assert_eq!(scheme, "bvxn-LZVN");
        assert_eq!(decoded, plaintext);
    }

    #[test]
    fn malformed_lzss_is_rejected() {
        assert!(lzss(b"\0\0").is_none());
    }
}
