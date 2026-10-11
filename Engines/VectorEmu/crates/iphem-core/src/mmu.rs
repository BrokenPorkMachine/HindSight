//! Minimal architectural Stage-1 and Stage-2 AArch64 VMSA walkers. 4 KiB and 16 KiB granule, 4 levels Stage-1, 3 levels Stage-2.
use crate::{
    bus::{Bus, MemTransaction, SecurityState, TransactionKind},
    trace::TraceEngine,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmuFault {
    Disabled,
    Translation {
        level: u8,
        stage2: bool,
        address: u64,
    },
    Permission {
        level: u8,
        stage2: bool,
        address: u64,
    },
    AccessFlag {
        level: u8,
        stage2: bool,
        address: u64,
    },
    InvalidDescriptor {
        level: u8,
        stage2: bool,
        address: u64,
    },
    AddressSize {
        level: u8,
        stage2: bool,
        address: u64,
    },
    Stage2TableWalk {
        level: u8,
        address: u64,
        kind: TableWalkFaultKind,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableWalkFaultKind {
    Translation,
    Permission,
    AccessFlag,
    InvalidDescriptor,
    AddressSize,
}
impl MmuFault {
    pub fn level(self) -> u8 {
        match self {
            Self::Disabled => 0,
            Self::Translation { level, .. }
            | Self::Permission { level, .. }
            | Self::AccessFlag { level, .. }
            | Self::InvalidDescriptor { level, .. }
            | Self::AddressSize { level, .. }
            | Self::Stage2TableWalk { level, .. } => level,
        }
    }

    pub fn is_stage2(self) -> bool {
        match self {
            Self::Disabled => false,
            Self::Translation { stage2, .. }
            | Self::Permission { stage2, .. }
            | Self::AccessFlag { stage2, .. }
            | Self::InvalidDescriptor { stage2, .. }
            | Self::AddressSize { stage2, .. } => stage2,
            Self::Stage2TableWalk { .. } => true,
        }
    }

    pub fn fault_address(self) -> Option<u64> {
        match self {
            Self::Disabled => None,
            Self::Translation { address, .. }
            | Self::Permission { address, .. }
            | Self::AccessFlag { address, .. }
            | Self::InvalidDescriptor { address, .. }
            | Self::AddressSize { address, .. }
            | Self::Stage2TableWalk { address, .. } => Some(address),
        }
    }

    pub fn is_stage2_table_walk(self) -> bool {
        matches!(self, Self::Stage2TableWalk { .. })
    }

    fn into_stage2_table_walk(self) -> Self {
        let (level, address, kind) = match self {
            Self::Translation {
                level,
                stage2: true,
                address,
            } => (level, address, TableWalkFaultKind::Translation),
            Self::Permission {
                level,
                stage2: true,
                address,
            } => (level, address, TableWalkFaultKind::Permission),
            Self::AccessFlag {
                level,
                stage2: true,
                address,
            } => (level, address, TableWalkFaultKind::AccessFlag),
            Self::InvalidDescriptor {
                level,
                stage2: true,
                address,
            } => (level, address, TableWalkFaultKind::InvalidDescriptor),
            Self::AddressSize {
                level,
                stage2: true,
                address,
            } => (level, address, TableWalkFaultKind::AddressSize),
            _ => return self,
        };
        Self::Stage2TableWalk {
            level,
            address,
            kind,
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Translation {
    pub pa: u64,
    pub attr_index: u8,
    pub writable: bool,
    pub executable: bool,
    pub global: bool,
}
#[derive(Debug, Clone, Copy)]
struct Stage2Translation {
    pa: u64,
    writable: bool,
    executable: bool,
}
pub struct Mmu;
impl Mmu {
    fn physical_address_bits(encoding: u64) -> Option<u32> {
        match encoding & 0x7 {
            0 => Some(32),
            1 => Some(36),
            2 => Some(40),
            3 => Some(42),
            4 => Some(44),
            5 => Some(48),
            // This 4 KiB walker does not implement the LPA2 52-bit format.
            _ => None,
        }
    }

    pub fn stage2_walk(
        cpu: &crate::cpu::CpuState,
        bus: &mut Bus,
        trace: &mut TraceEngine,
        ipa: u64,
        write: bool,
    ) -> Result<u64, MmuFault> {
        Self::stage2_walk_access(cpu, bus, trace, ipa, write, false).map(|t| t.pa)
    }

    fn stage2_walk_access(
        cpu: &crate::cpu::CpuState,
        bus: &mut Bus,
        trace: &mut TraceEngine,
        ipa: u64,
        write: bool,
        execute: bool,
    ) -> Result<Stage2Translation, MmuFault> {
        // HCR_EL2.VM controls Stage-2 translation. VTCR_EL2 describes the
        // translation regime; none of its T0SZ bits are enable controls.
        if cpu.hcr_el2 & 1 == 0 {
            return Ok(Stage2Translation {
                pa: ipa,
                writable: true,
                executable: true,
            });
        }
        let t0sz = cpu.vtcr_el2 & 0x3f;
        let sl0 = (cpu.vtcr_el2 >> 6) & 0x3;
        let start_level = match sl0 {
            0 => 2,
            1 => 1,
            2 => 0,
            _ => {
                return Err(MmuFault::AddressSize {
                    level: 0,
                    stage2: true,
                    address: ipa,
                });
            }
        };
        let input_bits = 64u64.saturating_sub(t0sz);
        let Some(pa_bits) = Self::physical_address_bits((cpu.vtcr_el2 >> 16) & 0x7) else {
            return Err(MmuFault::AddressSize {
                level: 0,
                stage2: true,
                address: ipa,
            });
        };
        let expected_level = if input_bits > 39 {
            0
        } else if input_bits > 30 {
            1
        } else if input_bits > 21 {
            2
        } else {
            3
        };
        let granule_4k = (cpu.vtcr_el2 >> 14) & 0x3 == 0;
        if !granule_4k
            || !(22..=48).contains(&input_bits)
            || expected_level != start_level
            || (input_bits < 64 && ipa >> input_bits != 0)
        {
            return Err(MmuFault::AddressSize {
                level: 0,
                stage2: true,
                address: ipa,
            });
        }
        let mut table = cpu.vttbr_el2 & 0x0000_ffff_ffff_f000;
        if table >> pa_bits != 0 {
            return Err(MmuFault::AddressSize {
                level: start_level as u8,
                stage2: true,
                address: ipa,
            });
        }
        let va = ipa;
        for level in start_level..4u64 {
            let idx = match level {
                0 => (va >> 39) & 0x1ff,
                1 => (va >> 30) & 0x1ff,
                2 => (va >> 21) & 0x1ff,
                _ => (va >> 12) & 0x1ff,
            };
            let pa = table + idx * 8;
            let mut d = bus
                .read(
                    MemTransaction {
                        address: pa,
                        size: 8,
                        kind: TransactionKind::Read,
                        security: SecurityState::Secure,
                        source: None,
                    },
                    trace,
                    cpu.pc,
                    cpu.el as u8,
                    cpu.pstate,
                )
                .map_err(|_| MmuFault::Translation {
                    level: level as u8,
                    stage2: true,
                    address: ipa,
                })?;
            if d & 1 == 0 {
                return Err(MmuFault::InvalidDescriptor {
                    level: level as u8,
                    stage2: true,
                    address: ipa,
                });
            }
            let typ = (d >> 1) & 1;
            if typ == 1 && level < 3 {
                table = d & 0x0000_ffff_ffff_f000;
                if table >> pa_bits != 0 {
                    return Err(MmuFault::AddressSize {
                        level: level as u8,
                        stage2: true,
                        address: ipa,
                    });
                }
                continue;
            }
            let shift = if level == 3 {
                if typ != 1 {
                    return Err(MmuFault::InvalidDescriptor {
                        level: level as u8,
                        stage2: true,
                        address: ipa,
                    });
                }
                12
            } else {
                if level == 0 || typ != 0 {
                    return Err(MmuFault::InvalidDescriptor {
                        level: level as u8,
                        stage2: true,
                        address: ipa,
                    });
                }
                39 - 9 * level
            };
            let offset_mask = (1u64 << shift) - 1;
            let base = (d & 0x0000_ffff_ffff_f000) & !offset_mask;
            if base >> pa_bits != 0 {
                return Err(MmuFault::AddressSize {
                    level: level as u8,
                    stage2: true,
                    address: ipa,
                });
            }
            if d & (1 << 10) == 0 {
                if cpu.vtcr_el2 & (1 << 39) == 0 {
                    return Err(MmuFault::AccessFlag {
                        level: level as u8,
                        stage2: true,
                        address: ipa,
                    });
                }
                d |= 1 << 10;
                bus.write(
                    MemTransaction {
                        address: pa,
                        size: 8,
                        kind: TransactionKind::Write,
                        security: SecurityState::Secure,
                        source: None,
                    },
                    d,
                    trace,
                    cpu.pc,
                    cpu.el as u8,
                    cpu.pstate,
                )
                .map_err(|_| MmuFault::Translation {
                    level: level as u8,
                    stage2: true,
                    address: ipa,
                })?;
            }
            let mut s2ap = (d >> 6) & 0x3;
            let hardware_dirty = write
                && s2ap & 0x2 == 0
                && d & (1 << 51) != 0 // DBM
                && cpu.vtcr_el2 & ((1 << 39) | (1 << 40)) == ((1 << 39) | (1 << 40));
            if hardware_dirty {
                d |= 1 << 7; // S2AP[1] becomes the Stage-2 dirty state.
                bus.write(
                    MemTransaction {
                        address: pa,
                        size: 8,
                        kind: TransactionKind::Write,
                        security: SecurityState::Secure,
                        source: None,
                    },
                    d,
                    trace,
                    cpu.pc,
                    cpu.el as u8,
                    cpu.pstate,
                )
                .map_err(|_| MmuFault::Translation {
                    level: level as u8,
                    stage2: true,
                    address: ipa,
                })?;
                s2ap = (d >> 6) & 0x3;
            }
            let permitted = if write {
                s2ap & 0x2 != 0
            } else {
                s2ap & 0x1 != 0
            };
            let executable = d & (1 << 54) == 0;
            if !permitted || (execute && !executable) {
                return Err(MmuFault::Permission {
                    level: level as u8,
                    stage2: true,
                    address: ipa,
                });
            }
            return Ok(Stage2Translation {
                pa: base | (va & offset_mask),
                writable: s2ap & 0x2 != 0,
                executable,
            });
        }
        Err(MmuFault::Translation {
            level: 3,
            stage2: true,
            address: ipa,
        })
    }
    pub fn translate(
        cpu: &crate::cpu::CpuState,
        bus: &mut Bus,
        trace: &mut TraceEngine,
        va: u64,
        write: bool,
        execute: bool,
    ) -> Result<Translation, MmuFault> {
        Self::translate_inner(cpu, bus, trace, va, write, execute, true).map(|(t, _)| t)
    }

    /// Stage-1 AT query: retain Stage-2 translation of table walks, but return
    /// the output IPA without applying the final Stage-2 translation.
    /// The second result is the leaf's shareability encoding.
    pub fn translate_stage1(
        cpu: &crate::cpu::CpuState,
        bus: &mut Bus,
        trace: &mut TraceEngine,
        va: u64,
        write: bool,
    ) -> Result<(Translation, u8), MmuFault> {
        Self::translate_inner(cpu, bus, trace, va, write, false, false)
    }

    fn translate_inner(
        cpu: &crate::cpu::CpuState,
        bus: &mut Bus,
        trace: &mut TraceEngine,
        va: u64,
        write: bool,
        execute: bool,
        final_stage2: bool,
    ) -> Result<(Translation, u8), MmuFault> {
        if !cpu.state_sctlr_m() {
            let stage2 = if final_stage2 {
                Self::stage2_walk_access(cpu, bus, trace, va, write, execute)?
            } else {
                Stage2Translation {
                    pa: va,
                    writable: true,
                    executable: true,
                }
            };
            return Ok((
                Translation {
                    pa: stage2.pa,
                    attr_index: 0,
                    writable: stage2.writable,
                    executable: stage2.executable,
                    global: true,
                },
                0,
            ));
        }
        let tcr = cpu.tcr_el1;
        let Some(pa_bits) = Self::physical_address_bits((tcr >> 32) & 0x7) else {
            return Err(MmuFault::AddressSize {
                level: 0,
                stage2: false,
                address: va,
            });
        };
        // VA[55] remains the range selector when the top byte is ignored.
        let upper = va & (1 << 55) != 0;
        let tbi = if upper {
            tcr & (1 << 38) != 0
        } else {
            tcr & (1 << 37) != 0
        };
        let v = if tbi {
            (va & 0x00ff_ffff_ffff_ffff) | if upper { 0xff00_0000_0000_0000 } else { 0 }
        } else {
            va
        };
        let (ttbr, tnsz, tg, walk_disabled) = if upper {
            (
                cpu.ttbr1_el1,
                (tcr >> 16) & 0x3f,
                (tcr >> 30) & 0x3,
                tcr & (1 << 23) != 0, // EPD1
            )
        } else {
            (
                cpu.ttbr0_el1,
                tcr & 0x3f,
                (tcr >> 14) & 0x3,
                tcr & (1 << 7) != 0, // EPD0
            )
        };
        // TG0 and TG1 encode granule size differently:
        //   TG0 (bits 15:14): 0b00=4KB, 0b01=64KB, 0b10=16KB
        //   TG1 (bits 31:30): 0b10=4KB, 0b01=16KB, 0b11=64KB
        // Normalize TG1 → TG0 encoding so the match works for both halves.
        let tg_norm = if upper {
            match tg {
                0b10 => 0u64, // TG1 4KB → TG0 4KB
                0b01 => 2,    // TG1 16KB → TG0 16KB
                0b11 => 1,    // TG1 64KB → TG0 64KB
                _ => tg,
            }
        } else {
            tg
        };
        let (index_bits, page_shift) = match tg_norm {
            0 => (9u64, 12u64),  // 4 KB
            2 => (11u64, 14u64), // 16 KB
            _ => {
                return Err(MmuFault::AddressSize {
                    level: 0,
                    stage2: false,
                    address: va,
                });
            }
        };
        let input_bits = 64u64.saturating_sub(tnsz);
        if !(25..=48).contains(&input_bits) {
            return Err(MmuFault::AddressSize {
                level: 0,
                stage2: false,
                address: va,
            });
        }
        let canonical = if upper {
            (v as i64 >> input_bits) == -1
        } else {
            v >> input_bits == 0
        };
        if !canonical {
            return Err(MmuFault::AddressSize {
                level: 0,
                stage2: false,
                address: va,
            });
        }
        if walk_disabled {
            return Err(MmuFault::Translation {
                level: 0,
                stage2: false,
                address: va,
            });
        }
        let start_level = if input_bits > page_shift + index_bits * 3 {
            0
        } else if input_bits > page_shift + index_bits * 2 {
            1
        } else if input_bits > page_shift + index_bits {
            2
        } else {
            3
        };
        let mut table = ttbr & 0x0000_ffff_ffff_f000;
        if table >> pa_bits != 0 {
            return Err(MmuFault::AddressSize {
                level: start_level as u8,
                stage2: false,
                address: va,
            });
        }
        let mut ipa = va;
        let mut attr_index = 0u8;
        let mut shareability = 0u8;
        let mut writable = true;
        let mut executable = true;
        let mut global = true;
        let mut table_no_el0 = false;
        let mut table_read_only = false;
        let mut table_uxn = false;
        let mut table_pxn = false;
        let index_mask = (1u64 << index_bits) - 1;
        for level in start_level..4u64 {
            let shift = page_shift + index_bits * (3 - level);
            // A shortened root table indexes only the remaining VA bits,
            // excluding the canonical upper address's sign extension.
            let mask = if level == start_level {
                (1u64 << (input_bits - shift)) - 1
            } else {
                index_mask
            };
            let idx = (v >> shift) & mask;
            let descriptor_ipa = table + idx * 8;
            let pa = Self::stage2_walk_access(cpu, bus, trace, descriptor_ipa, false, false)
                .map_err(MmuFault::into_stage2_table_walk)?
                .pa;
            let mut d = bus
                .read(
                    MemTransaction {
                        address: pa,
                        size: 8,
                        kind: TransactionKind::Read,
                        security: SecurityState::Secure,
                        source: None,
                    },
                    trace,
                    cpu.pc,
                    cpu.el as u8,
                    cpu.pstate,
                )
                .map_err(|_| MmuFault::Translation {
                    level: level as u8,
                    stage2: false,
                    address: va,
                })?;
            if d & 1 == 0 {
                return Err(MmuFault::InvalidDescriptor {
                    level: level as u8,
                    stage2: false,
                    address: va,
                });
            }
            let typ = (d >> 1) & 1;
            if typ == 1 && level < 3 {
                let ap_table = (d >> 61) & 0x3;
                table_no_el0 |= ap_table & 0x1 != 0;
                table_read_only |= ap_table & 0x2 != 0;
                table_uxn |= d & (1 << 60) != 0;
                table_pxn |= d & (1 << 59) != 0;
                table = d & 0x0000_ffff_ffff_f000;
                if table >> pa_bits != 0 {
                    return Err(MmuFault::AddressSize {
                        level: level as u8,
                        stage2: false,
                        address: va,
                    });
                }
                continue;
            }
            let shift = if level == 3 {
                if typ != 1 {
                    return Err(MmuFault::InvalidDescriptor {
                        level: level as u8,
                        stage2: false,
                        address: va,
                    });
                }
                page_shift
            } else {
                if level == 0 || typ != 0 {
                    return Err(MmuFault::InvalidDescriptor {
                        level: level as u8,
                        stage2: false,
                        address: va,
                    });
                }
                page_shift + index_bits * (3 - level)
            };
            let offset_mask = (1u64 << shift) - 1;
            let base = (d & 0x0000_ffff_ffff_f000) & !offset_mask;
            if base >> pa_bits != 0 {
                return Err(MmuFault::AddressSize {
                    level: level as u8,
                    stage2: false,
                    address: va,
                });
            }
            ipa = base | (v & offset_mask);
            attr_index = ((d >> 2) & 7) as u8;
            shareability = ((d >> 8) & 3) as u8;
            let mut ap = ((d >> 6) & 0x3) as u8;
            let unprivileged = cpu.el == crate::cpu::ExceptionLevel::El0;
            let user_mapping = ap & 0x1 != 0 && !table_no_el0;
            let pan_blocks =
                !unprivileged && !execute && cpu.pstate & (1 << 22) != 0 && user_mapping;
            let accessible = (!unprivileged || user_mapping) && !pan_blocks;
            let hardware_dirty = write
                && accessible
                && !table_read_only
                && ap & 0x2 != 0
                && d & (1 << 51) != 0 // DBM
                && cpu.tcr_el1 & ((1 << 39) | (1 << 40)) == ((1 << 39) | (1 << 40));
            if hardware_dirty {
                let update_pa =
                    Self::stage2_walk_access(cpu, bus, trace, descriptor_ipa, true, false)
                        .map_err(MmuFault::into_stage2_table_walk)?
                        .pa;
                d &= !(1 << 7); // AP[2] becomes the hardware dirty state.
                bus.write(
                    MemTransaction {
                        address: update_pa,
                        size: 8,
                        kind: TransactionKind::Write,
                        security: SecurityState::Secure,
                        source: None,
                    },
                    d,
                    trace,
                    cpu.pc,
                    cpu.el as u8,
                    cpu.pstate,
                )
                .map_err(|_| MmuFault::Translation {
                    level: level as u8,
                    stage2: false,
                    address: va,
                })?;
                ap = ((d >> 6) & 0x3) as u8;
            }
            writable = accessible && ap & 0x2 == 0 && !table_read_only;
            let xn = if unprivileged {
                d & (1 << 54) != 0 || table_uxn // UXN
            } else {
                d & (1 << 53) != 0 || table_pxn // PXN
            };
            executable = !xn;
            global = d & (1 << 11) == 0; // nG
            if !accessible || (write && !writable) {
                return Err(MmuFault::Permission {
                    level: level as u8,
                    stage2: false,
                    address: va,
                });
            }
            if execute && xn {
                return Err(MmuFault::Permission {
                    level: level as u8,
                    stage2: false,
                    address: va,
                });
            }
            if d & (1 << 10) == 0 {
                if cpu.tcr_el1 & (1 << 39) == 0 {
                    return Err(MmuFault::AccessFlag {
                        level: level as u8,
                        stage2: false,
                        address: va,
                    });
                }
                // TCR_EL1.HA requests an architectural descriptor update.
                // The update is itself a Stage-1 table-walk write and must be
                // permitted by Stage-2 when virtualization is active.
                let update_pa =
                    Self::stage2_walk_access(cpu, bus, trace, descriptor_ipa, true, false)
                        .map_err(MmuFault::into_stage2_table_walk)?
                        .pa;
                d |= 1 << 10;
                bus.write(
                    MemTransaction {
                        address: update_pa,
                        size: 8,
                        kind: TransactionKind::Write,
                        security: SecurityState::Secure,
                        source: None,
                    },
                    d,
                    trace,
                    cpu.pc,
                    cpu.el as u8,
                    cpu.pstate,
                )
                .map_err(|_| MmuFault::Translation {
                    level: level as u8,
                    stage2: false,
                    address: va,
                })?;
            }
            break;
        }
        let stage2 = if final_stage2 {
            Mmu::stage2_walk_access(cpu, bus, trace, ipa, write, execute)?
        } else {
            Stage2Translation {
                pa: ipa,
                writable: true,
                executable: true,
            }
        };
        Ok((
            Translation {
                pa: stage2.pa,
                attr_index,
                writable: writable && stage2.writable,
                executable: executable && stage2.executable,
                global,
            },
            shareability,
        ))
    }
}
trait Sctlr {
    fn state_sctlr_m(&self) -> bool;
}
impl Sctlr for crate::cpu::CpuState {
    fn state_sctlr_m(&self) -> bool {
        self.sctlr_el1 & 1 != 0
    }
}
