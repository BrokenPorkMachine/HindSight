//! APFS container reader (Phase 160c, minimal subset).
//!
//! Read-only APFS container reader for synthesized boot images. Implements
//! just enough to validate the NX superblock and resolve a file lookup
//! against a synthetic directory layout (see `phase160.rs`).
//!
//! Not a complete APFS implementation: snapshots, clones, encryption,
//! multi-volume, and B-tree node splits are out of scope.

pub const BLOCK_SIZE: usize = 4096;

const NXSB_MAGIC: [u8; 4] = *b"NXSB";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApfsError {
    NotApfs,
    BadBlock,
    NotFound,
}

fn read_u16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn read_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn read_u64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes([
        b[o],
        b[o + 1],
        b[o + 2],
        b[o + 3],
        b[o + 4],
        b[o + 5],
        b[o + 6],
        b[o + 7],
    ])
}

/// A read-only view over a flat APFS container image.
pub struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn from_bytes(bytes: &'a [u8]) -> Result<Self, ApfsError> {
        if bytes.len() < BLOCK_SIZE {
            return Err(ApfsError::BadBlock);
        }
        if bytes[0..4] != NXSB_MAGIC {
            return Err(ApfsError::NotApfs);
        }
        Ok(Self { bytes })
    }

    pub fn block_count(&self) -> usize {
        self.bytes.len() / BLOCK_SIZE
    }

    fn block(&self, i: usize) -> Option<&[u8]> {
        let start = i * BLOCK_SIZE;
        let end = start + BLOCK_SIZE;
        self.bytes.get(start..end)
    }

    pub fn container_oid(&self) -> u64 {
        read_u64(self.bytes, 0x30)
    }

    pub fn apfs_root_oid(&self) -> Result<u64, ApfsError> {
        let b1 = self.block(1).ok_or(ApfsError::BadBlock)?;
        if &b1[0..4] != b"APSB" {
            return Err(ApfsError::NotApfs);
        }
        Ok(read_u64(b1, 0x28))
    }

    /// Read a file from the synthetic directory. The directory at block 4
    /// contains a list of entries:
    ///   u16 name_len, name (name_len bytes), u32 block_offset, u32 len
    /// The file at `path` is returned as a sub-slice of the original image.
    pub fn read_file(&self, path: &str) -> Result<&'a [u8], ApfsError> {
        let root = self.block(4).ok_or(ApfsError::BadBlock)?;
        let n = read_u32(root, 0) as usize;
        let mut cursor = 4usize;
        for _ in 0..n {
            if cursor + 2 > BLOCK_SIZE {
                return Err(ApfsError::BadBlock);
            }
            let name_len = read_u16(root, cursor) as usize;
            cursor += 2;
            if cursor + name_len + 8 > BLOCK_SIZE {
                return Err(ApfsError::BadBlock);
            }
            let name = &root[cursor..cursor + name_len];
            cursor += name_len;
            let payload_off = read_u32(root, cursor) as usize;
            cursor += 4;
            let payload_len = read_u32(root, cursor) as usize;
            cursor += 4;
            if name == path.as_bytes() {
                let start = payload_off * BLOCK_SIZE;
                let end = start + payload_len;
                if end > self.bytes.len() {
                    return Err(ApfsError::BadBlock);
                }
                return Ok(&self.bytes[start..end]);
            }
        }
        Err(ApfsError::NotFound)
    }
}
