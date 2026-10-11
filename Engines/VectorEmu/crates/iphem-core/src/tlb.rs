use crate::mmu::Translation;
#[derive(Clone, Copy, PartialEq, Eq)]
struct FastKey {
    va_page: u64,
    asid: u16,
    vmid: u16,
    unprivileged: bool,
}

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct TlbEntry {
    pub va_page: u64,
    pub pa_page: u64,
    pub asid: u16,
    pub vmid: u16,
    pub unprivileged: bool,
    pub global: bool,
    pub writable: bool,
    pub executable: bool,
    pub attr_index: u8,
}

/// Small direct-mapped cache mirroring the most recent TLB inserts. The linear
/// scan in `Tlb::lookup` is always the authority: a cache entry is only used
/// when its full key matches, so a stale or collided slot simply falls back to
/// the walk. Cleared on every eviction/invalidation to stay consistent.
struct FastTlb {
    slots: Vec<(FastKey, Translation)>,
}

impl FastTlb {
    fn sentinel(key_part: u16) -> FastKey {
        FastKey {
            va_page: u64::MAX,
            asid: key_part,
            vmid: key_part,
            unprivileged: !matches!(key_part & 1, 1),
        }
    }
    fn new(n: usize) -> Self {
        let key = Self::sentinel(0);
        let t = Translation {
            pa: 0,
            attr_index: 0,
            writable: false,
            executable: false,
            global: false,
        };
        Self {
            slots: (0..n.max(1)).map(|_| (key, t)).collect(),
        }
    }
    fn idx(&self, k: &FastKey) -> usize {
        let h = k.va_page.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ ((k.asid as u64) << 48)
            ^ ((k.vmid as u64) << 32)
            ^ u64::from(k.unprivileged);
        (h >> 32) as usize & (self.slots.len() - 1)
    }
    fn fill_none(&mut self) {
        let key = Self::sentinel(0);
        for s in &mut self.slots {
            s.0 = key;
        }
    }
}

pub struct Tlb {
    entries: Vec<TlbEntry>,
    capacity: usize,
    fast: FastTlb,
}
impl Tlb {
    fn va_page(va: u64) -> u64 {
        let canonical = (va & 0x00ff_ffff_ffff_ffff)
            | if va & (1 << 55) != 0 {
                0xff00_0000_0000_0000
            } else {
                0
            };
        canonical >> 12
    }

    /// A read access accepts both writable and read-only mappings; write/execute
    /// intents additionally filter on the entry's permission bits, mirroring the
    /// predicate used by the authoritative linear scan.
    fn accepts(e: &TlbEntry, write: bool, execute: bool) -> bool {
        (!write || e.writable) && (!execute || e.executable)
    }

    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            capacity: capacity.max(1),
            fast: FastTlb::new(256),
        }
    }
    pub fn lookup(
        &self,
        va: u64,
        asid: u16,
        vmid: u16,
        unprivileged: bool,
        write: bool,
        execute: bool,
    ) -> Option<Translation> {
        let p = Self::va_page(va);
        let key = FastKey {
            va_page: p,
            asid,
            vmid,
            unprivileged,
        };
        if let Some((k, t)) = self.fast.slots.get(self.fast.idx(&key)) {
            if *k == key && (!write || t.writable) && (!execute || t.executable) {
                return Some(Translation {
                    pa: (t.pa & !0xfff) | (va & 0xfff),
                    attr_index: t.attr_index,
                    writable: t.writable,
                    executable: t.executable,
                    global: t.global,
                });
            }
        }
        self.entries
            .iter()
            .rev()
            .find(|e| {
                e.va_page == p
                    && (e.global || e.asid == asid)
                    && e.vmid == vmid
                    && e.unprivileged == unprivileged
                    && Self::accepts(e, write, execute)
            })
            .map(|e| Translation {
                pa: (e.pa_page << 12) | (va & 0xfff),
                attr_index: e.attr_index,
                writable: e.writable,
                executable: e.executable,
                global: e.global,
            })
    }
    pub fn insert(&mut self, va: u64, asid: u16, vmid: u16, unprivileged: bool, t: Translation) {
        if self.entries.len() >= self.capacity {
            self.entries.remove(0);
            self.fast.fill_none();
        }
        let entry = TlbEntry {
            va_page: Self::va_page(va),
            pa_page: t.pa >> 12,
            asid,
            vmid,
            unprivileged,
            global: t.global,
            writable: t.writable,
            executable: t.executable,
            attr_index: t.attr_index,
        };
        self.entries.push(entry);
        let key = FastKey {
            va_page: entry.va_page,
            asid,
            vmid,
            unprivileged,
        };
        let idx = self.fast.idx(&key);
        self.fast.slots[idx] = (key, t);
    }
    fn clear_fast(&mut self) {
        self.fast.fill_none();
    }
    pub fn invalidate_all(&mut self) {
        self.entries.clear();
        self.clear_fast();
    }
    pub fn invalidate_va(&mut self, va: u64) {
        let p = Self::va_page(va);
        self.entries.retain(|e| e.va_page != p);
        self.clear_fast();
    }
    pub fn invalidate_va_asid(&mut self, va: u64, asid: u16, vmid: u16) {
        let p = Self::va_page(va);
        self.entries
            .retain(|e| e.va_page != p || e.vmid != vmid || (!e.global && e.asid != asid));
        self.clear_fast();
    }
    pub fn invalidate_va_vmid(&mut self, va: u64, vmid: u16) {
        let p = Self::va_page(va);
        self.entries.retain(|e| e.va_page != p || e.vmid != vmid);
        self.clear_fast();
    }
    pub fn invalidate_asid(&mut self, asid: u16, vmid: u16) {
        self.entries
            .retain(|e| e.global || e.asid != asid || e.vmid != vmid);
        self.clear_fast();
    }
    pub fn invalidate_vmid(&mut self, vmid: u16) {
        self.entries.retain(|e| e.vmid != vmid);
        self.clear_fast();
    }
    pub fn get_entries(&self) -> &[TlbEntry] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignored_top_byte_is_not_part_of_tlb_identity() {
        let mut tlb = Tlb::new(4);
        tlb.insert(
            0xab00_0000_0000_1234,
            0,
            0,
            false,
            Translation {
                pa: 0x8234,
                attr_index: 0,
                writable: true,
                executable: true,
                global: true,
            },
        );
        assert_eq!(
            tlb.lookup(0xcd00_0000_0000_1234, 0, 0, false, false, false)
                .unwrap()
                .pa,
            0x8234
        );
        tlb.invalidate_va(0x1234);
        assert!(tlb.get_entries().is_empty());
    }
}
