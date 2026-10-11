use crate::device::{Device, DeviceId, DeviceResult};
use std::any::Any;
use std::collections::BTreeMap;

/// Byte-exact evidence of writes routed through the guest bus. Host-side image
/// loading does not mark these bits, even when it populates the same RAM.
#[derive(Default)]
struct GuestWrites {
    pages: BTreeMap<u64, Box<[u64; 64]>>,
}

impl GuestWrites {
    fn mark(&mut self, offset: u64, size: usize) {
        let end = offset + size as u64;
        let mut cursor = offset;
        while cursor < end {
            let page = cursor >> 12;
            let page_end = end.min((page + 1).saturating_mul(4096));
            let start = (cursor & 0xfff) as usize;
            let stop = start + (page_end - cursor) as usize;
            let bits = self.pages.entry(page).or_insert_with(|| Box::new([0; 64]));
            for word in start / 64..=(stop - 1) / 64 {
                let low = if word == start / 64 { start % 64 } else { 0 };
                let high = if word == (stop - 1) / 64 {
                    (stop - 1) % 64 + 1
                } else {
                    64
                };
                let left = u64::MAX << low;
                let right = if high == 64 {
                    u64::MAX
                } else {
                    (1u64 << high) - 1
                };
                bits[word] |= left & right;
            }
            cursor = page_end;
        }
    }

    fn intersects(&self, offset: u64, len: usize) -> bool {
        let Some(end) = offset.checked_add(len as u64) else {
            return false;
        };
        if offset >= end {
            return false;
        }
        self.pages
            .range((offset >> 12)..=((end - 1) >> 12))
            .any(|(&page, bits)| {
                let start = offset.saturating_sub(page << 12).min(4096) as usize;
                let stop = end.saturating_sub(page << 12).min(4096) as usize;
                (start / 64..=(stop - 1) / 64).any(|word| {
                    let low = if word == start / 64 { start % 64 } else { 0 };
                    let high = if word == (stop - 1) / 64 {
                        (stop - 1) % 64 + 1
                    } else {
                        64
                    };
                    let left = u64::MAX << low;
                    let right = if high == 64 {
                        u64::MAX
                    } else {
                        (1u64 << high) - 1
                    };
                    bits[word] & left & right != 0
                })
            })
    }
}

pub struct Ram {
    pub base: u64,
    pub data: Vec<u8>,
    guest_writes: GuestWrites,
}
impl Ram {
    pub fn new(base: u64, size: usize) -> Self {
        Self {
            base,
            data: vec![0; size],
            guest_writes: GuestWrites::default(),
        }
    }
    pub fn load(&mut self, off: usize, b: &[u8]) -> bool {
        match off.checked_add(b.len()) {
            Some(e) if e <= self.data.len() => {
                self.data[off..e].copy_from_slice(b);
                true
            }
            _ => false,
        }
    }
    pub fn size(&self) -> u64 {
        self.data.len() as u64
    }

    pub fn guest_wrote_range(&self, offset: u64, len: usize) -> bool {
        self.guest_writes.intersects(offset, len)
    }

    /// Copy bytes from RAM for host-side devices such as framebuffer scanout.
    pub fn copy_out(&self, offset: u64, out: &mut [u8]) -> bool {
        let Some(start) = usize::try_from(offset).ok() else {
            return false;
        };
        let Some(end) = start.checked_add(out.len()) else {
            return false;
        };
        if end > self.data.len() {
            return false;
        }
        out.copy_from_slice(&self.data[start..end]);
        true
    }
}
impl Device for Ram {
    fn id(&self) -> DeviceId {
        0
    }
    fn name(&self) -> &'static str {
        "dram"
    }
    fn reset(&mut self) {
        self.data.fill(0);
        self.guest_writes = GuestWrites::default();
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        let o = o as usize;
        if !matches!(s, 1 | 2 | 4 | 8)
            || o.checked_add(s)
                .map(|e| e > self.data.len())
                .unwrap_or(true)
        {
            return DeviceResult::Fault;
        };
        let mut v = 0;
        for i in 0..s {
            v |= (self.data[o + i] as u64) << (i * 8)
        }
        DeviceResult::Value(v)
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        let o = o as usize;
        if !matches!(s, 1 | 2 | 4 | 8)
            || o.checked_add(s)
                .map(|e| e > self.data.len())
                .unwrap_or(true)
        {
            return DeviceResult::Fault;
        };
        for i in 0..s {
            self.data[o + i] = (v >> (i * 8)) as u8
        }
        self.guest_writes.mark(o as u64, s);
        DeviceResult::Ok
    }
    fn tick(&mut self, _: u64) -> Option<crate::device::DeviceEvent> {
        None
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub struct Rom {
    pub base: u64,
    pub data: Vec<u8>,
}
impl Rom {
    pub fn new(base: u64, data: Vec<u8>) -> Self {
        Self { base, data }
    }
}
impl Device for Rom {
    fn id(&self) -> DeviceId {
        0
    }
    fn name(&self) -> &'static str {
        "rom"
    }
    fn reset(&mut self) {}
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        let o = o as usize;
        if !matches!(s, 1 | 2 | 4 | 8)
            || o.checked_add(s)
                .map(|e| e > self.data.len())
                .unwrap_or(true)
        {
            return DeviceResult::Fault;
        };
        let mut v = 0;
        for i in 0..s {
            v |= (self.data[o + i] as u64) << (i * 8)
        }
        DeviceResult::Value(v)
    }
    fn write(&mut self, _: u64, _: usize, _: u64) -> DeviceResult {
        DeviceResult::Fault
    }
    fn tick(&mut self, _: u64) -> Option<crate::device::DeviceEvent> {
        None
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub struct AddressSpace {
    pub ram: Ram,
    pub roms: Vec<Rom>,
}
impl AddressSpace {
    pub fn new(ram: Ram) -> Self {
        Self {
            ram,
            roms: Vec::new(),
        }
    }
}

/// Sparse guest RAM: address space can be multi-gigabyte without allocating
/// host RAM up front. Unwritten pages read as zero and are allocated on write.
pub struct SparseRam {
    pub base: u64,
    pub size: u64,
    pages: std::collections::BTreeMap<u64, Box<[u8; 4096]>>,
    guest_writes: GuestWrites,
}
impl SparseRam {
    pub fn new(base: u64, size: u64) -> Self {
        Self {
            base,
            size,
            pages: std::collections::BTreeMap::new(),
            guest_writes: GuestWrites::default(),
        }
    }
    fn check(&self, o: u64, s: usize) -> bool {
        o.checked_add(s as u64)
            .map(|e| e <= self.size)
            .unwrap_or(false)
    }
    fn page_mut(&mut self, p: u64) -> &mut [u8; 4096] {
        self.pages.entry(p).or_insert_with(|| Box::new([0; 4096]))
    }
    pub fn get_pages(&self) -> &std::collections::BTreeMap<u64, Box<[u8; 4096]>> {
        &self.pages
    }

    pub fn guest_wrote_range(&self, offset: u64, len: usize) -> bool {
        self.guest_writes.intersects(offset, len)
    }

    /// Read a guest physical-memory range without issuing one bus transaction
    /// per byte. Unallocated sparse pages are architecturally zero-filled.
    pub fn copy_out(&self, offset: u64, out: &mut [u8]) -> bool {
        let Some(end) = offset.checked_add(out.len() as u64) else {
            return false;
        };
        if end > self.size {
            return false;
        }
        let mut address = offset;
        let mut copied = 0;
        while copied < out.len() {
            let page = address >> 12;
            let within = (address & 0xfff) as usize;
            let count = (out.len() - copied).min(4096 - within);
            if let Some(data) = self.pages.get(&page) {
                out[copied..copied + count].copy_from_slice(&data[within..within + count]);
            } else {
                out[copied..copied + count].fill(0);
            }
            copied += count;
            address += count as u64;
        }
        true
    }

    /// Copy a host-provided image into sparse RAM without routing every byte
    /// through the guest bus. Intended for firmware loading before execution.
    pub fn load(&mut self, offset: u64, mut data: &[u8]) -> bool {
        let Some(end) = offset.checked_add(data.len() as u64) else {
            return false;
        };
        if end > self.size {
            return false;
        }

        let mut address = offset;
        while !data.is_empty() {
            let page = address >> 12;
            let within = (address & 0xfff) as usize;
            let count = data.len().min(4096 - within);
            self.page_mut(page)[within..within + count].copy_from_slice(&data[..count]);
            address += count as u64;
            data = &data[count..];
        }
        true
    }
}
impl Device for SparseRam {
    fn id(&self) -> DeviceId {
        0
    }
    fn name(&self) -> &'static str {
        "sparse-dram"
    }
    fn reset(&mut self) {
        self.pages.clear();
        self.guest_writes = GuestWrites::default();
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) || !self.check(o, s) {
            return DeviceResult::Fault;
        }
        let p = o >> 12;
        let q = (o & 0xfff) as usize;
        // Fast path: the whole access stays inside one page -> one map lookup.
        if q + s <= 4096 {
            let Some(page) = self.pages.get(&p) else {
                return DeviceResult::Value(0);
            };
            let mut v = 0;
            for i in 0..s {
                v |= (page[q + i] as u64) << (i * 8);
            }
            return DeviceResult::Value(v);
        }
        // Cross-page fallback (a 1/2/4/8-byte access can cross at most once).
        let mut v = 0;
        for i in 0..s {
            let a = o + i as u64;
            let pp = a >> 12;
            let qq = (a & 0xfff) as usize;
            let b = self.pages.get(&pp).map(|x| x[qq]).unwrap_or(0);
            v |= (b as u64) << (i * 8);
        }
        DeviceResult::Value(v)
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) || !self.check(o, s) {
            return DeviceResult::Fault;
        }
        let p = o >> 12;
        let q = (o & 0xfff) as usize;
        // Fast path: single page, single map lookup, straight byte store.
        if q + s <= 4096 {
            let page = self.page_mut(p);
            for i in 0..s {
                page[q + i] = (v >> (i * 8)) as u8;
            }
            self.guest_writes.mark(o, s);
            return DeviceResult::Ok;
        }
        // Cross-page fallback (a 1/2/4/8-byte access can cross at most once).
        for i in 0..s {
            let a = o + i as u64;
            let pp = a >> 12;
            let qq = (a & 0xfff) as usize;
            self.page_mut(pp)[qq] = (v >> (i * 8)) as u8;
        }
        self.guest_writes.mark(o, s);
        DeviceResult::Ok
    }
    fn tick(&mut self, _: u64) -> Option<crate::device::DeviceEvent> {
        None
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_ram_bulk_load_crosses_page_boundary() {
        let mut ram = SparseRam::new(0, 0x3000);
        let data: Vec<u8> = (0..64).map(|value| value as u8).collect();
        assert!(ram.load(0xff0, &data));
        for (index, expected) in data.iter().enumerate() {
            assert_eq!(
                ram.read(0xff0 + index as u64, 1),
                DeviceResult::Value(u64::from(*expected))
            );
        }
        assert!(!ram.load(0x2ff0, &data));
    }

    #[test]
    fn sparse_ram_copy_out_reads_allocated_and_zero_pages() {
        let mut ram = SparseRam::new(0, 0x3000);
        assert!(ram.load(0xffe, &[0xaa, 0xbb, 0xcc, 0xdd]));
        let mut out = [0xff; 8];
        assert!(ram.copy_out(0xffe, &mut out));
        assert_eq!(out, [0xaa, 0xbb, 0xcc, 0xdd, 0, 0, 0, 0]);
        assert!(!ram.copy_out(0x2ffc, &mut out));
    }

    #[test]
    fn guest_write_evidence_excludes_host_load_and_tracks_exact_bytes() {
        let mut sparse = SparseRam::new(0, 0x3000);
        assert!(sparse.load(0xffe, &[1, 2, 3, 4]));
        assert!(!sparse.guest_wrote_range(0xffe, 4));
        assert_eq!(sparse.write(0xfff, 4, 0), DeviceResult::Ok);
        assert!(!sparse.guest_wrote_range(0xffe, 1));
        assert!(sparse.guest_wrote_range(0xfff, 4));
        assert!(!sparse.guest_wrote_range(0x1003, 1));
        sparse.reset();
        assert!(!sparse.guest_wrote_range(0xfff, 4));

        let mut ram = Ram::new(0, 0x3000);
        assert!(ram.load(0xffe, &[1, 2, 3, 4]));
        assert!(!ram.guest_wrote_range(0xffe, 4));
        assert_eq!(ram.write(0xfff, 4, 0), DeviceResult::Ok);
        assert!(!ram.guest_wrote_range(0xffe, 1));
        assert!(ram.guest_wrote_range(0xfff, 4));
        assert!(!ram.guest_wrote_range(0x1003, 1));
    }
}
