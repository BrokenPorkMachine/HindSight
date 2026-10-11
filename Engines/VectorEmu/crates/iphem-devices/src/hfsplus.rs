//! HFS+ (Mac OS Extended) filesystem reader for iOS rootfs images.
//!
//! Bounded read-only implementation for extracting files from iOS rootfs images.
//! Supports volume validation, catalog CNID/path traversal, data/resource forks,
//! and extent-overflow continuation without fabricating missing file content.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const HFS_PLUS_MAGIC: u16 = 0x482b; // 'H+'
const HFSX_MAGIC: u16 = 0x4858; // 'HX'
const HFS_EXTENTS_FILE_ID: u32 = 3;
const HFS_CATALOG_FILE_ID: u32 = 4;
const HFS_DATA_FORK: u8 = 0;
const HFS_BINARY_COMPARE: u8 = 0xbc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HfsError {
    NotHfs,
    InvalidHeader,
    BTreeError,
    NotFound,
    IoError,
    Truncated,
}

#[derive(Debug, Clone)]
struct HfsPlusVolumeHeader {
    signature: u16,
    version: u16,
    attributes: u32,
    last_mounted_version: u32,
    journal_info_block: u32,
    create_date: u32,
    modify_date: u32,
    backup_date: u32,
    checked_date: u32,
    file_count: u32,
    folder_count: u32,
    block_size: u32,
    total_blocks: u32,
    free_blocks: u32,
    next_allocation: u32,
    rsrc_clump_sz: u32,
    data_clump_sz: u32,
    next_cnid: u32,
    write_count: u32,
    encodings_bitmap: u64,
    finder_info: [u32; 8],
    allocation_file: HfsForkData,
    extents_file: HfsForkData,
    catalog_file: HfsForkData,
    attributes_file: HfsForkData,
    startup_file: HfsForkData,
}

#[derive(Debug, Clone)]
pub struct HfsForkData {
    logical_size: u64,
    clump_size: u32,
    total_blocks: u32,
    extents: [HfsExtentDescriptor; 8],
}

#[derive(Debug, Clone, Copy)]
struct HfsExtentDescriptor {
    start_block: u32,
    block_count: u32,
}

#[derive(Debug, Clone)]
enum CatalogRecordKind {
    Folder,
    File {
        data_fork: HfsForkData,
        resource_fork: HfsForkData,
    },
}

#[derive(Debug, Clone)]
struct CatalogRecord {
    parent_id: u32,
    name: String,
    cnid: u32,
    kind: CatalogRecordKind,
}

/// Read-only HFS+ filesystem accessor
pub struct HfsPlusReader {
    volume: HfsPlusVolumeHeader,
    file: File,
    block_size: u32,
}

impl HfsPlusReader {
    /// Open an HFS+ volume whose HFS+ header begins at the standard offset 1024.
    pub fn open(file: File) -> Result<Self, HfsError> {
        let mut reader = file;
        // HFS+ volume header is at block 2 (offset 1024 bytes for 512-byte blocks)
        reader
            .seek(SeekFrom::Start(1024))
            .map_err(|_| HfsError::IoError)?;
        let mut header_buf = [0u8; 512];
        reader
            .read_exact(&mut header_buf)
            .map_err(|_| HfsError::IoError)?;

        if header_buf[0..2] != HFS_PLUS_MAGIC.to_be_bytes()
            && header_buf[0..2] != HFSX_MAGIC.to_be_bytes()
        {
            return Err(HfsError::NotHfs);
        }

        let volume = Self::parse_volume_header(&header_buf)?;
        let block_size = volume.block_size;
        if block_size < 512 || !block_size.is_power_of_two() || volume.total_blocks == 0 {
            return Err(HfsError::InvalidHeader);
        }
        let declared_bytes = u64::from(block_size)
            .checked_mul(u64::from(volume.total_blocks))
            .ok_or(HfsError::InvalidHeader)?;
        if declared_bytes > reader.metadata().map_err(|_| HfsError::IoError)?.len() {
            return Err(HfsError::Truncated);
        }

        Ok(HfsPlusReader {
            volume,
            file: reader,
            block_size,
        })
    }

    fn parse_volume_header(data: &[u8]) -> Result<HfsPlusVolumeHeader, HfsError> {
        if data.len() < 512 {
            return Err(HfsError::Truncated);
        }
        let mut offset = 0;
        let signature = u16::from_be_bytes([data[offset], data[offset + 1]]);
        offset += 2;
        let version = u16::from_be_bytes([data[offset], data[offset + 1]]);
        offset += 2;
        let attributes = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        let last_mounted_version = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        let journal_info_block = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        let create_date = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        let modify_date = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        let backup_date = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        let checked_date = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        let file_count = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        let folder_count = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        let block_size = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        let total_blocks = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        let free_blocks = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        let next_allocation = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        let rsrc_clump_sz = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        let data_clump_sz = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        let next_cnid = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        let write_count = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        let encodings_bitmap = u64::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]);
        offset += 8;
        let mut finder_info = [0u32; 8];
        for i in 0..8 {
            finder_info[i] = u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]);
            offset += 4;
        }

        let allocation_file = Self::parse_fork_data(&data[offset..])?;
        offset += 80;
        let extents_file = Self::parse_fork_data(&data[offset..])?;
        offset += 80;
        let catalog_file = Self::parse_fork_data(&data[offset..])?;
        offset += 80;
        let attributes_file = Self::parse_fork_data(&data[offset..])?;
        offset += 80;
        let startup_file = Self::parse_fork_data(&data[offset..])?;

        Ok(HfsPlusVolumeHeader {
            signature,
            version,
            attributes,
            last_mounted_version,
            journal_info_block,
            create_date,
            modify_date,
            backup_date,
            checked_date,
            file_count,
            folder_count,
            block_size,
            total_blocks,
            free_blocks,
            next_allocation,
            rsrc_clump_sz,
            data_clump_sz,
            next_cnid,
            write_count,
            encodings_bitmap,
            finder_info,
            allocation_file,
            extents_file,
            catalog_file,
            attributes_file,
            startup_file,
        })
    }

    fn parse_fork_data(data: &[u8]) -> Result<HfsForkData, HfsError> {
        if data.len() < 80 {
            return Err(HfsError::Truncated);
        }
        let mut offset = 0;
        let logical_size = u64::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]);
        offset += 8;
        let clump_size = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        let total_blocks = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        let mut extents = [HfsExtentDescriptor {
            start_block: 0,
            block_count: 0,
        }; 8];
        for i in 0..8 {
            if offset + 8 > data.len() {
                return Err(HfsError::Truncated);
            }
            extents[i] = HfsExtentDescriptor {
                start_block: u32::from_be_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]),
                block_count: u32::from_be_bytes([
                    data[offset + 4],
                    data[offset + 5],
                    data[offset + 6],
                    data[offset + 7],
                ]),
            };
            offset += 8;
        }
        Ok(HfsForkData {
            logical_size,
            clump_size,
            total_blocks,
            extents,
        })
    }

    fn be_u16(data: &[u8], offset: usize) -> Result<u16, HfsError> {
        let bytes = data.get(offset..offset + 2).ok_or(HfsError::Truncated)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn be_u32(data: &[u8], offset: usize) -> Result<u32, HfsError> {
        let bytes = data.get(offset..offset + 4).ok_or(HfsError::Truncated)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn btree_node_size(data: &[u8]) -> Result<usize, HfsError> {
        // Header node descriptor is 14 bytes. BTHeaderRec.nodeSize is 18 bytes
        // into the first header record, which conventionally starts at byte 14.
        if data.len() < 34 || data[8] as i8 != 1 {
            return Err(HfsError::BTreeError);
        }
        let node_size = usize::from(Self::be_u16(data, 32)?);
        if !(512..=32768).contains(&node_size) || !node_size.is_power_of_two() {
            return Err(HfsError::BTreeError);
        }
        if data.len() < node_size {
            return Err(HfsError::Truncated);
        }
        Ok(node_size)
    }

    fn btree_key_compare_type(data: &[u8]) -> Option<u8> {
        // BTHeaderRec.keyCompareType is byte 37 of the record at offset 14.
        data.get(51).copied()
    }

    fn for_each_leaf_record<F>(data: &[u8], mut visit: F) -> Result<(), HfsError>
    where
        F: FnMut(&[u8]) -> Result<(), HfsError>,
    {
        let node_size = Self::btree_node_size(data)?;
        if data.len() % node_size != 0 {
            return Err(HfsError::Truncated);
        }
        for node in data.chunks_exact(node_size) {
            if node.len() < 14 || node[8] as i8 != -1 {
                continue;
            }
            let record_count = usize::from(Self::be_u16(node, 10)?);
            let offset_bytes = (record_count + 1)
                .checked_mul(2)
                .ok_or(HfsError::BTreeError)?;
            if offset_bytes > node_size.saturating_sub(14) {
                return Err(HfsError::BTreeError);
            }
            let record_offset = |index: usize| -> Result<usize, HfsError> {
                let tail = (index + 1).checked_mul(2).ok_or(HfsError::BTreeError)?;
                let pos = node_size.checked_sub(tail).ok_or(HfsError::BTreeError)?;
                Ok(usize::from(Self::be_u16(node, pos)?))
            };
            for index in 0..record_count {
                let start = record_offset(index)?;
                let end = record_offset(index + 1)?;
                let offset_table_start = node_size - offset_bytes;
                if start < 14 || start >= end || end > offset_table_start {
                    return Err(HfsError::BTreeError);
                }
                visit(&node[start..end])?;
            }
        }
        Ok(())
    }

    fn decode_catalog_name(record: &[u8], key_end: usize) -> Result<String, HfsError> {
        let units = usize::from(Self::be_u16(record, 6)?);
        let bytes = units.checked_mul(2).ok_or(HfsError::BTreeError)?;
        let end = 8usize.checked_add(bytes).ok_or(HfsError::BTreeError)?;
        if end > key_end || end > record.len() {
            return Err(HfsError::Truncated);
        }
        let mut utf16 = Vec::with_capacity(units);
        for pair in record[8..end].chunks_exact(2) {
            utf16.push(u16::from_be_bytes([pair[0], pair[1]]));
        }
        String::from_utf16(&utf16).map_err(|_| HfsError::BTreeError)
    }

    fn parse_catalog_records(data: &[u8]) -> Result<Vec<CatalogRecord>, HfsError> {
        let mut records = Vec::new();
        Self::for_each_leaf_record(data, |record| {
            let key_len = usize::from(Self::be_u16(record, 0)?);
            let key_end = 2usize.checked_add(key_len).ok_or(HfsError::BTreeError)?;
            if key_len < 6 || key_end > record.len() {
                return Err(HfsError::BTreeError);
            }
            let parent_id = Self::be_u32(record, 2)?;
            let name = Self::decode_catalog_name(record, key_end)?;
            let value_offset = (key_end + 1) & !1;
            let record_type = Self::be_u16(record, value_offset)?;
            match record_type {
                1 => {
                    // HFSPlusCatalogFolder.folderID follows recordType, flags and valence.
                    let cnid = Self::be_u32(record, value_offset + 8)?;
                    records.push(CatalogRecord {
                        parent_id,
                        name,
                        cnid,
                        kind: CatalogRecordKind::Folder,
                    });
                }
                2 => {
                    // HFSPlusCatalogFile.fileID is at +8; the two 80-byte forks
                    // start at +88 and +168 respectively.
                    let cnid = Self::be_u32(record, value_offset + 8)?;
                    let data_start = value_offset.checked_add(88).ok_or(HfsError::BTreeError)?;
                    let rsrc_start = value_offset.checked_add(168).ok_or(HfsError::BTreeError)?;
                    let data_fork = Self::parse_fork_data(
                        record.get(data_start..).ok_or(HfsError::Truncated)?,
                    )?;
                    let resource_fork = Self::parse_fork_data(
                        record.get(rsrc_start..).ok_or(HfsError::Truncated)?,
                    )?;
                    records.push(CatalogRecord {
                        parent_id,
                        name,
                        cnid,
                        kind: CatalogRecordKind::File {
                            data_fork,
                            resource_fork,
                        },
                    });
                }
                3 | 4 => {
                    // Folder/file thread records are reverse-name metadata and do
                    // not participate in forward path traversal.
                }
                _ => return Err(HfsError::BTreeError),
            }
            Ok(())
        })?;
        Ok(records)
    }

    fn parse_overflow_extents(
        data: &[u8],
        file_id: u32,
        fork_type: u8,
    ) -> Result<Vec<(u32, [HfsExtentDescriptor; 8])>, HfsError> {
        let mut records = Vec::new();
        Self::for_each_leaf_record(data, |record| {
            let key_len = usize::from(Self::be_u16(record, 0)?);
            let key_end = 2usize.checked_add(key_len).ok_or(HfsError::BTreeError)?;
            if key_len < 10 || key_end > record.len() {
                return Err(HfsError::BTreeError);
            }
            let key_fork = *record.get(2).ok_or(HfsError::Truncated)?;
            let key_file = Self::be_u32(record, 4)?;
            if key_fork != fork_type || key_file != file_id {
                return Ok(());
            }
            let start_block = Self::be_u32(record, 8)?;
            let mut value_offset = (key_end + 1) & !1;
            let mut extents = [HfsExtentDescriptor {
                start_block: 0,
                block_count: 0,
            }; 8];
            for extent in &mut extents {
                extent.start_block = Self::be_u32(record, value_offset)?;
                extent.block_count = Self::be_u32(record, value_offset + 4)?;
                value_offset += 8;
            }
            records.push((start_block, extents));
            Ok(())
        })?;
        records.sort_by_key(|(start, _)| *start);
        Ok(records)
    }

    fn inline_block_count(fork: &HfsForkData) -> u64 {
        fork.extents
            .iter()
            .map(|extent| u64::from(extent.block_count))
            .sum()
    }

    fn fork_extents(
        &mut self,
        file_id: u32,
        fork_type: u8,
        fork: &HfsForkData,
    ) -> Result<Vec<HfsExtentDescriptor>, HfsError> {
        let mut extents: Vec<_> = fork
            .extents
            .iter()
            .copied()
            .filter(|extent| extent.block_count != 0)
            .collect();
        let mut covered: u64 = extents
            .iter()
            .map(|extent| u64::from(extent.block_count))
            .sum();
        if covered >= u64::from(fork.total_blocks) {
            return Ok(extents);
        }
        if file_id == HFS_EXTENTS_FILE_ID {
            // The extents-overflow file cannot recursively describe overflow of
            // itself without a bootstrap extent map. Fail closed rather than
            // silently returning a truncated fork.
            return Err(HfsError::BTreeError);
        }
        let extents_fork = self.volume.extents_file.clone();
        if Self::inline_block_count(&extents_fork) < u64::from(extents_fork.total_blocks) {
            return Err(HfsError::BTreeError);
        }
        let overflow_data = self.read_fork_data_basic(&extents_fork)?;
        for (logical_start, record_extents) in
            Self::parse_overflow_extents(&overflow_data, file_id, fork_type)?
        {
            if u64::from(logical_start) < covered {
                continue;
            }
            if u64::from(logical_start) != covered {
                return Err(HfsError::BTreeError);
            }
            for extent in record_extents {
                if extent.block_count == 0 {
                    continue;
                }
                covered = covered
                    .checked_add(u64::from(extent.block_count))
                    .ok_or(HfsError::BTreeError)?;
                extents.push(extent);
                if covered >= u64::from(fork.total_blocks) {
                    return Ok(extents);
                }
            }
        }
        if covered < u64::from(fork.total_blocks) {
            return Err(HfsError::BTreeError);
        }
        Ok(extents)
    }

    fn read_extents(
        &mut self,
        extents: &[HfsExtentDescriptor],
        logical_size: u64,
    ) -> Result<Vec<u8>, HfsError> {
        let logical_len = usize::try_from(logical_size).map_err(|_| HfsError::InvalidHeader)?;
        let file_len = self.file.metadata().map_err(|_| HfsError::IoError)?.len();
        let mut data = Vec::new();
        data.try_reserve_exact(logical_len)
            .map_err(|_| HfsError::IoError)?;
        let mut remaining = logical_size;
        for extent in extents {
            if remaining == 0 {
                break;
            }
            let start_byte = u64::from(extent.start_block)
                .checked_mul(u64::from(self.block_size))
                .ok_or(HfsError::InvalidHeader)?;
            let extent_bytes = u64::from(extent.block_count)
                .checked_mul(u64::from(self.block_size))
                .ok_or(HfsError::InvalidHeader)?;
            let read_bytes = extent_bytes.min(remaining);
            let end = start_byte
                .checked_add(read_bytes)
                .ok_or(HfsError::InvalidHeader)?;
            if end > file_len {
                return Err(HfsError::Truncated);
            }
            let read_len = usize::try_from(read_bytes).map_err(|_| HfsError::InvalidHeader)?;
            self.file
                .seek(SeekFrom::Start(start_byte))
                .map_err(|_| HfsError::IoError)?;
            let old_len = data.len();
            data.resize(old_len + read_len, 0);
            self.file
                .read_exact(&mut data[old_len..])
                .map_err(|_| HfsError::IoError)?;
            remaining -= read_bytes;
        }
        if remaining != 0 {
            return Err(HfsError::BTreeError);
        }
        Ok(data)
    }

    fn read_fork_data_basic(&mut self, fork: &HfsForkData) -> Result<Vec<u8>, HfsError> {
        let extents: Vec<_> = fork
            .extents
            .iter()
            .copied()
            .filter(|extent| extent.block_count != 0)
            .collect();
        self.read_extents(&extents, fork.logical_size)
    }

    fn read_fork_data_with_overflow(
        &mut self,
        file_id: u32,
        fork_type: u8,
        fork: &HfsForkData,
    ) -> Result<Vec<u8>, HfsError> {
        if fork.logical_size == 0 {
            return Ok(Vec::new());
        }
        let extents = self.fork_extents(file_id, fork_type, fork)?;
        self.read_extents(&extents, fork.logical_size)
    }

    fn names_equal(case_sensitive: bool, left: &str, right: &str) -> bool {
        if case_sensitive {
            left == right
        } else {
            left.to_lowercase() == right.to_lowercase()
        }
    }

    /// Read a regular file by absolute or root-relative path from the HFS+
    /// catalog. Path lookup walks catalog CNIDs; file bytes come from the data
    /// fork and continue through the extents-overflow B-tree when required.
    pub fn read_file(&mut self, path: &str) -> Result<Vec<u8>, HfsError> {
        let components: Vec<_> = path
            .split('/')
            .filter(|component| !component.is_empty())
            .collect();
        if components.is_empty()
            || components
                .iter()
                .any(|component| *component == "." || *component == "..")
        {
            return Err(HfsError::NotFound);
        }

        let catalog_fork = self.volume.catalog_file.clone();
        let catalog_data = self.read_fork_data_with_overflow(
            HFS_CATALOG_FILE_ID,
            HFS_DATA_FORK,
            &catalog_fork,
        )?;
        let case_sensitive =
            Self::btree_key_compare_type(&catalog_data) == Some(HFS_BINARY_COMPARE);
        let records = Self::parse_catalog_records(&catalog_data)?;

        let mut parent_id = 2u32; // kHFSRootFolderID
        for (index, component) in components.iter().enumerate() {
            let record = records
                .iter()
                .find(|record| {
                    record.parent_id == parent_id
                        && Self::names_equal(case_sensitive, &record.name, component)
                })
                .ok_or(HfsError::NotFound)?;
            let last = index + 1 == components.len();
            match (&record.kind, last) {
                (CatalogRecordKind::Folder, false) => parent_id = record.cnid,
                (CatalogRecordKind::File { data_fork, .. }, true) => {
                    return self.read_fork_data_with_overflow(
                        record.cnid,
                        HFS_DATA_FORK,
                        data_fork,
                    )
                }
                _ => return Err(HfsError::NotFound),
            }
        }
        Err(HfsError::NotFound)
    }

    /// Read a resource fork for a catalog file. This uses the same path/CNID
    /// validation as [`read_file`] and keeps resource-fork support explicit.
    pub fn read_resource_fork(&mut self, path: &str) -> Result<Vec<u8>, HfsError> {
        let components: Vec<_> = path
            .split('/')
            .filter(|component| !component.is_empty())
            .collect();
        if components.is_empty()
            || components
                .iter()
                .any(|component| *component == "." || *component == "..")
        {
            return Err(HfsError::NotFound);
        }
        let catalog_fork = self.volume.catalog_file.clone();
        let catalog_data = self.read_fork_data_with_overflow(
            HFS_CATALOG_FILE_ID,
            HFS_DATA_FORK,
            &catalog_fork,
        )?;
        let case_sensitive =
            Self::btree_key_compare_type(&catalog_data) == Some(HFS_BINARY_COMPARE);
        let records = Self::parse_catalog_records(&catalog_data)?;
        let mut parent_id = 2u32;
        for (index, component) in components.iter().enumerate() {
            let record = records
                .iter()
                .find(|record| {
                    record.parent_id == parent_id
                        && Self::names_equal(case_sensitive, &record.name, component)
                })
                .ok_or(HfsError::NotFound)?;
            let last = index + 1 == components.len();
            match (&record.kind, last) {
                (CatalogRecordKind::Folder, false) => parent_id = record.cnid,
                (CatalogRecordKind::File { resource_fork, .. }, true) => {
                    return self.read_fork_data_with_overflow(record.cnid, 0xff, resource_fork)
                }
                _ => return Err(HfsError::NotFound),
            }
        }
        Err(HfsError::NotFound)
    }

    pub fn block_size(&self) -> u32 {
        self.block_size
    }
}

/// Check if a file is HFS+ by reading its volume header
pub fn is_hfsplus(path: &Path) -> bool {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    if file.seek(SeekFrom::Start(1024)).is_err() {
        return false;
    }
    let mut signature = [0u8; 2];
    if file.read_exact(&mut signature).is_err() {
        return false;
    }
    signature == HFS_PLUS_MAGIC.to_be_bytes() || signature == HFSX_MAGIC.to_be_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;



    fn make_leaf_btree(record: &[u8], compare_type: u8) -> Vec<u8> {
        let node_size = 512usize;
        let mut data = vec![0u8; node_size * 2];
        data[8] = 1; // header node
        data[32..34].copy_from_slice(&(node_size as u16).to_be_bytes());
        data[51] = compare_type;
        let node = &mut data[node_size..];
        node[8] = 0xff; // kBTLeafNode
        node[10..12].copy_from_slice(&1u16.to_be_bytes());
        let start = 14usize;
        let end = start + record.len();
        node[start..end].copy_from_slice(record);
        node[node_size - 2..node_size].copy_from_slice(&(start as u16).to_be_bytes());
        node[node_size - 4..node_size - 2].copy_from_slice(&(end as u16).to_be_bytes());
        data
    }

    fn file_record(parent: u32, name: &str, cnid: u32, start_block: u32) -> Vec<u8> {
        let units: Vec<u16> = name.encode_utf16().collect();
        let key_len = 4 + 2 + units.len() * 2;
        let value_offset = (2 + key_len + 1) & !1;
        let mut record = vec![0u8; value_offset + 248];
        record[0..2].copy_from_slice(&(key_len as u16).to_be_bytes());
        record[2..6].copy_from_slice(&parent.to_be_bytes());
        record[6..8].copy_from_slice(&(units.len() as u16).to_be_bytes());
        for (index, unit) in units.iter().enumerate() {
            let at = 8 + index * 2;
            record[at..at + 2].copy_from_slice(&unit.to_be_bytes());
        }
        record[value_offset..value_offset + 2].copy_from_slice(&2u16.to_be_bytes());
        record[value_offset + 8..value_offset + 12].copy_from_slice(&cnid.to_be_bytes());
        let fork = value_offset + 88;
        record[fork..fork + 8].copy_from_slice(&4u64.to_be_bytes());
        record[fork + 12..fork + 16].copy_from_slice(&1u32.to_be_bytes());
        record[fork + 16..fork + 20].copy_from_slice(&start_block.to_be_bytes());
        record[fork + 20..fork + 24].copy_from_slice(&1u32.to_be_bytes());
        record
    }

    #[test]
    fn catalog_leaf_decodes_real_file_fork_metadata() {
        let record = file_record(2, "launchd", 42, 9);
        let tree = make_leaf_btree(&record, HFS_BINARY_COMPARE);
        let records = HfsPlusReader::parse_catalog_records(&tree).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].parent_id, 2);
        assert_eq!(records[0].name, "launchd");
        assert_eq!(records[0].cnid, 42);
        match &records[0].kind {
            CatalogRecordKind::File { data_fork, .. } => {
                assert_eq!(data_fork.logical_size, 4);
                assert_eq!(data_fork.total_blocks, 1);
                assert_eq!(data_fork.extents[0].start_block, 9);
            }
            _ => panic!("expected file record"),
        }
    }

    #[test]
    fn hfs_probe_checks_signature_not_just_seekability() {
        let base = std::env::temp_dir();
        let good = base.join(format!("vectorglass-hfs-good-{}", std::process::id()));
        let bad = base.join(format!("vectorglass-hfs-bad-{}", std::process::id()));
        let mut good_bytes = vec![0u8; 1536];
        good_bytes[1024..1026].copy_from_slice(&HFS_PLUS_MAGIC.to_be_bytes());
        std::fs::write(&good, good_bytes).unwrap();
        std::fs::write(&bad, vec![0u8; 1536]).unwrap();
        assert!(is_hfsplus(&good));
        assert!(!is_hfsplus(&bad));
        let _ = std::fs::remove_file(good);
        let _ = std::fs::remove_file(bad);
    }
    #[test]
    fn test_hfs_volume_header_parsing() {
        let mut header = vec![0u8; 512];
        header[0..2].copy_from_slice(&HFS_PLUS_MAGIC.to_be_bytes());
        header[2..4].copy_from_slice(&4u16.to_be_bytes()); // version
                                                           // Skip to block_size at offset 40
        header[40..44].copy_from_slice(&4096u32.to_be_bytes());
        header[44..48].copy_from_slice(&1000000u32.to_be_bytes()); // total_blocks

        let parsed = HfsPlusReader::parse_volume_header(&header).unwrap();
        assert_eq!(parsed.signature, HFS_PLUS_MAGIC);
        assert_eq!(parsed.block_size, 4096);
    }
}
