//! Foundational address types. No implicit conversion between address spaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImageOffset(pub u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RuntimeAddress(pub u64);

impl ImageOffset {
    pub fn checked_add(self, delta: u64) -> Option<Self> {
        self.0.checked_add(delta).map(Self)
    }
    pub fn to_hex(self) -> String { format!("0x{:016x}", self.0) }
    pub fn to_ordered_bytes(self) -> [u8; 8] { self.0.to_be_bytes() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn overflow_is_rejected() {
        assert_eq!(ImageOffset(u64::MAX).checked_add(1), None);
    }
    #[test] fn firmware_addresses_preserve_all_bits() {
        let value = ImageOffset(0xfffffff00a46c000);
        assert_eq!(value.to_hex(), "0xfffffff00a46c000");
        assert_eq!(u64::from_be_bytes(value.to_ordered_bytes()), value.0);
    }
    #[test] fn unsigned_sort_order_is_preserved() {
        assert!(ImageOffset(1).to_ordered_bytes() < ImageOffset(u64::MAX).to_ordered_bytes());
    }
}
