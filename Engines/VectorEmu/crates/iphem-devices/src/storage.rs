//! Storage (Phase 160)
//!
//! Register-level functional models for the storage stack:
//! - `EmmcSdHc` — SD/eMMC host controller (SDHCI-compatible register window)
//! - `NandArray` — multi-plane NAND flash backing store
//! - `ApfsReader` — minimal APFS container reader (read-only)

use iphem_core::device::{Device, DeviceEvent, DeviceId, DeviceResult};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

fn valid_size(s: usize) -> bool {
    matches!(s, 1 | 2 | 4 | 8)
}

fn read_zero(_o: u64, s: usize) -> DeviceResult {
    if valid_size(s) {
        DeviceResult::Value(0)
    } else {
        DeviceResult::Fault
    }
}

fn drop_write(_o: u64, s: usize, _v: u64) -> DeviceResult {
    if valid_size(s) {
        DeviceResult::Ok
    } else {
        DeviceResult::Fault
    }
}

// ----------------------------------------------------------------------------
// EmmcSdHc — SDHCI-compatible register window
// ----------------------------------------------------------------------------

/// Software-emulated SD card with up to N 512-byte blocks.
pub struct SdCard {
    pub blocks: Vec<[u8; 512]>,
    pub rca: u32,
    pub ocr: u32,
    pub csd: [u32; 4],
    pub cid: [u32; 4],
}

impl SdCard {
    pub fn new(block_count: usize) -> Self {
        let blocks = vec![[0u8; 512]; block_count];
        // CSD/CID use deterministic, card-shaped modeled identity fields; unsupported vendor-specific fields remain zero by contract.
        let csd = [0x400E_0032, 0x5B59_0000, 0x0000_0000, 0x0000_0000];
        let cid = [0xDEAD_BEEF, 0x0000_0001, 0x4150_504C, 0x45FE_0000];
        Self {
            blocks,
            rca: 0,
            ocr: 0xC0FF_8000,
            csd,
            cid,
        }
    }

    pub fn from_file(path: &Path) -> std::io::Result<Self> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        let block_count = (len as usize).div_ceil(512);
        let mut blocks = vec![[0u8; 512]; block_count];
        for block in blocks.iter_mut() {
            let n = file.read(block)?;
            if n == 0 {
                break;
            }
        }
        Ok(Self {
            blocks,
            rca: 0,
            ocr: 0xC0FF_8000,
            csd: [0x400E_0032, 0x5B59_0000, 0x0000_0000, 0x0000_0000],
            cid: [0xDEAD_BEEF, 0x0000_0001, 0x4150_504C, 0x45FE_0000],
        })
    }

    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    pub fn read_block(&self, lba: u32) -> Option<&[u8; 512]> {
        self.blocks.get(lba as usize)
    }

    pub fn write_block(&mut self, lba: u32, data: [u8; 512]) -> bool {
        if let Some(b) = self.blocks.get_mut(lba as usize) {
            *b = data;
            true
        } else {
            false
        }
    }

    pub fn is_ready(&self) -> bool {
        (self.ocr & 0x8000_0000) != 0
    }
}

pub struct EmmcSdHc {
    pub id: DeviceId,
    pub blksize: u16,
    pub blkcnt: u16,
    pub arg: u32,
    pub cmd: u32,
    pub resp: [u32; 4],
    pub data: [u8; 512],
    pub data_len: usize,
    pub present: u32,
    pub host_ctl: u8,
    pub power: u8,
    pub clk_ctl: u16,
    pub int_stat: u32,
    pub int_en: u32,
    pub sig_en: u32,
    pub err_stat: u16,
    pub card: SdCard,
    pub irq: u32,
    /// Compact status word describing the last command that ran:
    /// bit 0 = command failed (ADMA/CRC error seen on the data path),
    /// upper bits = the command index that produced the status.
    last_status: u32,
    fifo_off: usize,
}

impl EmmcSdHc {
    pub const INT_CMD_DONE: u32 = 1 << 0;
    pub const INT_DATA_DONE: u32 = 1 << 1;
    pub const INT_ERR: u32 = 1 << 5;

    /// Byte-FIFO window hosting the block-transfer buffer (`data`).
    pub const DATA_PORT: u64 = 0x48;

    pub fn new(id: DeviceId, card: SdCard) -> Self {
        Self {
            id,
            blksize: 0x0200,
            blkcnt: 0,
            arg: 0,
            cmd: 0,
            resp: [0; 4],
            data: [0; 512],
            data_len: 0,
            present: 0x0000_0001, // cmd line not inhibited
            host_ctl: 0,
            power: 0,
            clk_ctl: 0,
            int_stat: 0,
            int_en: 0,
            sig_en: 0,
            err_stat: 0,
            card,
            irq: 58,
            last_status: 0,
            fifo_off: 0,
        }
    }

    pub fn with_irq(id: DeviceId, card: SdCard, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id, card)
        }
    }

    fn execute_cmd(&mut self) {
        // Clear present cmd-inhibit; raise command-done after the (software)
        // response is computed.
        self.present &= !0x0000_0001;
        self.err_stat = 0;
        let idx = (self.cmd >> 24) & 0x3F;
        // Bit 5 (CMD.reg[21]) = data present.
        let data_present = (self.cmd & (1 << 21)) != 0;
        let mut handled_data = false;
        match idx {
            0 => {
                // CMD0 GO_IDLE_STATE
                self.resp = [0; 4];
            }
            1 => {
                // CMD1 SEND_OP_COND — host asks card to publish OCR.
                self.card.ocr |= 0x8000_0000; // power-up done
                self.resp[0] = self.card.ocr;
            }
            2 => {
                // CMD2 ALL_SEND_CID
                self.resp = self.card.cid;
            }
            3 => {
                // CMD3 SEND_RELATIVE_ADDR
                self.card.rca = 0x1234;
                self.resp[0] = self.card.rca << 16;
            }
            7 => {
                // CMD7 SELECT_CARD
                self.resp[0] = self.card.rca;
            }
            8 => {
                // CMD8 SEND_IF_COND
                self.resp[0] = self.arg;
            }
            9 => {
                // CMD9 SEND_CSD
                self.resp = self.card.csd;
            }
            13 => {
                // CMD13 SEND_STATUS
                self.resp[0] = if self.card.is_ready() {
                    0x0000_0900
                } else {
                    0x0000_0000
                }
            }
            17 => {
                // CMD17 READ_SINGLE_BLOCK — stage the LBA into the data FIFO.
                handled_data = true;
                match self.card.read_block(self.arg) {
                    Some(block) => {
                        self.data.copy_from_slice(block);
                        self.data_len = 512;
                        self.fifo_off = 0;
                        self.int_stat |= Self::INT_DATA_DONE;
                    }
                    None => {
                        self.err_stat |= 0x10;
                        self.int_stat |= Self::INT_ERR;
                    }
                }
            }
            24 => {
                // CMD24 WRITE_SINGLE_BLOCK — flush the data FIFO to the card.
                handled_data = true;
                let mut blk = [0u8; 512];
                let n = self.data_len.min(512);
                blk[..n].copy_from_slice(&self.data[..n]);
                if self.card.write_block(self.arg, blk) {
                    self.data_len = 0;
                    self.fifo_off = 0;
                    self.int_stat |= Self::INT_DATA_DONE;
                } else {
                    self.err_stat |= 0x10;
                    self.int_stat |= Self::INT_ERR;
                }
            }
            _ => {
                self.resp = [0; 4];
            }
        }
        if data_present && !handled_data {
            // Never report a successful transfer for an unimplemented data
            // command. Unsupported data commands fail explicitly so firmware
            // cannot mistake a no-op for completed I/O.
            self.err_stat |= 0x10;
            self.int_stat |= Self::INT_ERR;
        }
        self.int_stat |= Self::INT_CMD_DONE;
        self.last_status = (idx << 8) | u32::from(self.err_stat != 0);
    }

    pub fn read(&mut self, offset: u64, size: usize) -> DeviceResult {
        match offset {
            0 => read_zero(offset, size),
            4 => DeviceResult::Value(self.blksize as u64),
            8 => DeviceResult::Value(self.blkcnt as u64),
            12 => DeviceResult::Value(self.arg as u64),
            16 => DeviceResult::Value(self.cmd as u64),
            20..=32 => {
                let idx = (offset - 20) / 4;
                if idx < 4 {
                    DeviceResult::Value(self.resp[idx as usize] as u64)
                } else {
                    read_zero(offset, size)
                }
            }
            36 => DeviceResult::Value(self.data_len as u64),
            40 => DeviceResult::Value(self.present as u64),
            44 => DeviceResult::Value(self.host_ctl as u64),
            48 => DeviceResult::Value(self.power as u64),
            52 => DeviceResult::Value(self.clk_ctl as u64),
            Self::DATA_PORT => DeviceResult::Value(self.data_window_read(size)),
            0x4C => DeviceResult::Value(self.last_status as u64),
            56 => {
                let val = self.int_stat as u64;
                self.int_stat = 0;
                DeviceResult::Value(val)
            }
            60 => DeviceResult::Value(self.int_en as u64),
            64 => DeviceResult::Value(self.sig_en as u64),
            68 => DeviceResult::Value(self.err_stat as u64),
            _ => read_zero(offset, size),
        }
    }

    pub fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult {
        match offset {
            0 => drop_write(offset, size, value),
            4 => {
                self.blksize = value as u16;
                DeviceResult::Ok
            }
            8 => {
                self.blkcnt = value as u16;
                DeviceResult::Ok
            }
            12 => {
                self.arg = value as u32;
                DeviceResult::Ok
            }
            16 => {
                self.cmd = value as u32;
                self.execute_cmd();
                DeviceResult::Ok
            }
            44 => {
                self.power = value as u8;
                DeviceResult::Ok
            }
            52 => {
                self.clk_ctl = value as u16;
                DeviceResult::Ok
            }
            Self::DATA_PORT => {
                self.data_window_write(size, value);
                DeviceResult::Ok
            }
            0x4C => {
                // Software clears the host command-status word.
                self.last_status = value as u32;
                DeviceResult::Ok
            }
            60 => {
                self.int_en = value as u32;
                DeviceResult::Ok
            }
            64 => {
                self.sig_en = value as u32;
                DeviceResult::Ok
            }
            _ => drop_write(offset, size, value),
        }
    }
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn data_window_read(&mut self, size: usize) -> u64 {
        let avail = self.data_len.saturating_sub(self.fifo_off);
        if avail == 0 {
            return 0;
        }
        let n = size.min(avail).min(8);
        let mut out = 0u64;
        for i in 0..n {
            out |= (self.data[self.fifo_off + i] as u64) << (i * 8);
        }
        self.fifo_off += n;
        out
    }

    fn data_window_write(&mut self, size: usize, value: u64) {
        let n = size.min(8);
        let end = (self.fifo_off + n).min(self.data.len());
        for (i, slot) in (self.fifo_off..end).enumerate() {
            self.data[slot] = (value >> (i * 8)) as u8;
        }
        self.fifo_off = end;
        self.data_len = self.data_len.max(end);
    }

    fn reset(&mut self) {
        self.blksize = 0x0200;
        self.blkcnt = 0;
        self.arg = 0;
        self.cmd = 0;
        self.resp = [0; 4];
        self.data.fill(0);
        self.data_len = 0;
        self.present = 0x0000_0001;
        self.host_ctl = 0;
        self.power = 0;
        self.clk_ctl = 0;
        self.int_stat = 0;
        self.int_en = 0;
        self.sig_en = 0;
        self.err_stat = 0;
        self.last_status = 0;
        self.fifo_off = 0;
    }
    fn tick(&mut self, _cycles: u64) -> Option<DeviceEvent> {
        if self.int_stat != 0 {
            Some(DeviceEvent::Interrupt(self.irq))
        } else {
            None
        }
    }
    fn as_any(&self) -> &(dyn std::any::Any + 'static) {
        self
    }
    fn as_any_mut(&mut self) -> &mut (dyn std::any::Any + 'static) {
        self
    }
}

impl Device for EmmcSdHc {
    fn id(&self) -> u32 {
        self.id
    }
    fn name(&self) -> &'static str {
        "EmmcSdHc"
    }
    fn read(&mut self, offset: u64, size: usize) -> DeviceResult {
        self.read(offset, size)
    }
    fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult {
        self.write(offset, size, value)
    }
    fn reset(&mut self) {
        self.reset()
    }
    fn tick(&mut self, cycles: u64) -> Option<DeviceEvent> {
        self.tick(cycles)
    }
    fn as_any(&self) -> &(dyn std::any::Any + 'static) {
        self.as_any()
    }
    fn as_any_mut(&mut self) -> &mut (dyn std::any::Any + 'static) {
        self.as_any_mut()
    }
}

// ----------------------------------------------------------------------------
// NandArray — multi-plane NAND flash backing store
// ----------------------------------------------------------------------------

pub const NAND_BLOCK_SIZE: usize = 128 * 1024; // 128 KiB erase block
pub const NAND_PAGE_SIZE: usize = 4096; // 4 KiB page
pub const NAND_SPARE_SIZE: usize = 128;
pub const NAND_PAGES_PER_BLOCK: usize = NAND_BLOCK_SIZE / NAND_PAGE_SIZE;

pub struct NandArray {
    pub blocks: Vec<[u8; NAND_BLOCK_SIZE]>,
    pub bad: Vec<bool>,
    backing_file: Option<File>,
    read_cache: [u8; NAND_PAGE_SIZE],
}

impl NandArray {
    pub fn new(block_count: usize) -> Self {
        Self {
            blocks: vec![[0xFFu8; NAND_BLOCK_SIZE]; block_count],
            bad: vec![false; block_count],
            backing_file: None,
            read_cache: [0xFF; NAND_PAGE_SIZE],
        }
    }

    /// Open a persistent flat NAND image without copying it into host memory.
    /// The last partial erase block reads as erased (`0xFF`) past EOF.
    pub fn open_file(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let len = file.metadata()?.len() as usize;
        let block_count = len.div_ceil(NAND_BLOCK_SIZE);
        Ok(Self {
            blocks: Vec::new(),
            bad: vec![false; block_count],
            backing_file: Some(file),
            read_cache: [0xFF; NAND_PAGE_SIZE],
        })
    }

    pub fn from_image(bytes: &[u8]) -> Self {
        let block_count = (bytes.len() + NAND_BLOCK_SIZE - 1) / NAND_BLOCK_SIZE.max(1);
        let mut a = Self::new(block_count.max(1));
        let _copy_len = bytes.len().min(a.blocks.len() * NAND_BLOCK_SIZE);
        for (i, b) in a.blocks.iter_mut().enumerate() {
            let start = i * NAND_BLOCK_SIZE;
            let end = (start + NAND_BLOCK_SIZE).min(bytes.len());
            if start < bytes.len() {
                let len = end - start;
                b[..len].copy_from_slice(&bytes[start..end]);
            }
        }
        a
    }

    pub fn block_count(&self) -> usize {
        if self.backing_file.is_some() {
            self.bad.len()
        } else {
            self.blocks.len()
        }
    }

    pub fn is_bad(&self, block: usize) -> bool {
        self.bad.get(block).copied().unwrap_or(true)
    }

    pub fn read_page(&mut self, block: usize, page: usize) -> Option<&[u8]> {
        if self.is_bad(block) {
            return None;
        }
        if page >= NAND_PAGES_PER_BLOCK {
            return None;
        }
        let off = page * NAND_PAGE_SIZE;
        if let Some(file) = self.backing_file.as_mut() {
            self.read_cache.fill(0xFF);
            let absolute = block.checked_mul(NAND_BLOCK_SIZE)?.checked_add(off)? as u64;
            file.seek(SeekFrom::Start(absolute)).ok()?;
            let mut filled = 0usize;
            while filled < NAND_PAGE_SIZE {
                match file.read(&mut self.read_cache[filled..]) {
                    Ok(0) => break,
                    Ok(n) => filled += n,
                    Err(_) => return None,
                }
            }
            return Some(&self.read_cache);
        }
        self.blocks
            .get(block)
            .map(|b| &b[off..off + NAND_PAGE_SIZE])
    }

    pub fn program_page(&mut self, block: usize, page: usize, data: &[u8]) -> bool {
        if self.is_bad(block) {
            return false;
        }
        if page >= NAND_PAGES_PER_BLOCK {
            return false;
        }
        if data.len() > NAND_PAGE_SIZE {
            return false;
        }
        let off = page * NAND_PAGE_SIZE;
        if let Some(file) = self.backing_file.as_mut() {
            let absolute = match block
                .checked_mul(NAND_BLOCK_SIZE)
                .and_then(|value| value.checked_add(off))
            {
                Some(value) => value as u64,
                None => return false,
            };
            return file.seek(SeekFrom::Start(absolute)).is_ok() && file.write_all(data).is_ok();
        }
        if let Some(b) = self.blocks.get_mut(block) {
            b[off..off + data.len()].copy_from_slice(data);
            true
        } else {
            false
        }
    }

    pub fn erase_block(&mut self, block: usize) -> bool {
        if self.is_bad(block) {
            return false;
        }
        if let Some(file) = self.backing_file.as_mut() {
            let absolute = match block.checked_mul(NAND_BLOCK_SIZE) {
                Some(value) => value as u64,
                None => return false,
            };
            let erased = vec![0xFF; NAND_BLOCK_SIZE];
            return file.seek(SeekFrom::Start(absolute)).is_ok() && file.write_all(&erased).is_ok();
        }
        if let Some(b) = self.blocks.get_mut(block) {
            for v in b.iter_mut() {
                *v = 0xFF;
            }
            true
        } else {
            false
        }
    }

    pub fn mark_bad(&mut self, block: usize) {
        if let Some(b) = self.bad.get_mut(block) {
            *b = true;
        }
    }
}

#[cfg(test)]
mod storage_tests {
    use super::*;

    #[test]
    fn emmc_commands_execute_on_write_and_use_card_backing() {
        let mut card = SdCard::new(2);
        for (i, byte) in card.blocks[0].iter_mut().enumerate() {
            *byte = (i & 0xff) as u8;
        }
        let mut host = EmmcSdHc::new(7, card);

        assert_eq!(host.write(12, 4, 0), DeviceResult::Ok);
        assert_eq!(host.write(16, 4, (17u64 << 24) | (1 << 21)), DeviceResult::Ok);
        assert_ne!(host.int_stat & EmmcSdHc::INT_CMD_DONE, 0);
        assert_ne!(host.int_stat & EmmcSdHc::INT_DATA_DONE, 0);
        assert_eq!(host.data_len, 512);
        assert_eq!(host.read(EmmcSdHc::DATA_PORT, 4), DeviceResult::Value(0x0302_0100));

        host.data = [0x5a; 512];
        host.data_len = 512;
        host.fifo_off = 0;
        assert_eq!(host.write(12, 4, 1), DeviceResult::Ok);
        assert_eq!(host.write(16, 4, (24u64 << 24) | (1 << 21)), DeviceResult::Ok);
        assert_eq!(host.card.read_block(1).unwrap(), &[0x5a; 512]);
    }

    #[test]
    fn unsupported_data_command_fails_instead_of_faking_completion() {
        let mut host = EmmcSdHc::new(7, SdCard::new(1));
        assert_eq!(host.write(16, 4, (18u64 << 24) | (1 << 21)), DeviceResult::Ok);
        assert_ne!(host.int_stat & EmmcSdHc::INT_ERR, 0);
        assert_eq!(host.int_stat & EmmcSdHc::INT_DATA_DONE, 0);
        assert_ne!(host.err_stat, 0);
    }

    #[test]
    fn emmc_reset_clears_controller_state_without_erasing_card() {
        let mut card = SdCard::new(1);
        card.blocks[0][0] = 0xa5;
        let mut host = EmmcSdHc::new(7, card);
        host.arg = 9;
        host.cmd = 0xffff_ffff;
        host.data_len = 123;
        host.int_stat = u32::MAX;
        host.reset();
        assert_eq!(host.arg, 0);
        assert_eq!(host.cmd, 0);
        assert_eq!(host.data_len, 0);
        assert_eq!(host.int_stat, 0);
        assert_eq!(host.card.blocks[0][0], 0xa5);
    }
}
