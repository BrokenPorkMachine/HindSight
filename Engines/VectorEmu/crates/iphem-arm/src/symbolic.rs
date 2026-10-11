//! Opt-in symbolic provenance primitives for firmware research.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr {
    Const(u64),
    Input(&'static str),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Shl(Box<Expr>, u8),
    Ubfm { value: Box<Expr>, lsb: u8, msb: u8 },
    Load { address: u64, width: u8 },
    Unknown { pc: u64, width: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferClass {
    MovZ,
    MovK,
    AddSub,
    Unknown,
}

#[derive(Clone, Debug)]
pub struct RegisterSnapshot {
    pub pc: u64,
    pub opcode: u32,
    pub regs: [u64; 31],
    pub sp: u64,
    pub pstate: u64,
    pub el: u8,
}

#[derive(Debug)]
pub struct RegisterCollector {
    start: u64,
    end: u64,
    limit: usize,
    snapshots: Vec<RegisterSnapshot>,
}

impl RegisterCollector {
    pub fn new(start: u64, end: u64, limit: usize) -> Self {
        Self {
            start,
            end,
            limit,
            snapshots: Vec::new(),
        }
    }
    pub fn observe(&mut self, snapshot: RegisterSnapshot) {
        if (self.start..=self.end).contains(&snapshot.pc) && self.snapshots.len() < self.limit {
            if self
                .snapshots
                .last()
                .is_some_and(|last| last.pc == snapshot.pc)
            {
                *self.snapshots.last_mut().expect("last snapshot exists") = snapshot;
                return;
            }
            self.snapshots.push(snapshot);
        } else if matches!(snapshot.pc, 0x1000_0856c | 0x1000_08570) {
            if let Some(slot) = self
                .snapshots
                .iter_mut()
                .find(|entry| entry.pc == snapshot.pc)
            {
                *slot = snapshot;
            } else if !self.snapshots.is_empty() {
                self.snapshots[0] = snapshot;
            }
        }
    }
    pub fn snapshots(&self) -> &[RegisterSnapshot] {
        &self.snapshots
    }
}

pub fn classify_transfer(op: u32) -> TransferClass {
    if op & 0x7f80_0000 == 0x5280_0000 {
        TransferClass::MovZ
    } else if op & 0x7f80_0000 == 0x7280_0000 {
        TransferClass::MovK
    } else if op & 0x1f00_0000 == 0x1100_0000 {
        TransferClass::AddSub
    } else {
        TransferClass::Unknown
    }
}

pub fn apply_movz_encoding(session: &mut SymbolicSession, pc: u64, op: u32) -> bool {
    if classify_transfer(op) != TransferClass::MovZ {
        return false;
    }
    let rd = (op & 0x1f) as usize;
    let imm = ((op >> 5) & 0xffff) as u64;
    let shift = ((op >> 21) & 0x3) * 16;
    session.apply_movz(pc, rd, imm << shift);
    true
}

pub fn apply_movk_encoding(session: &mut SymbolicSession, pc: u64, op: u32) -> bool {
    if classify_transfer(op) != TransferClass::MovK {
        return false;
    }
    let rd = (op & 0x1f) as usize;
    let imm = u64::from((op >> 5) & 0xffff);
    let shift = ((op >> 21) & 0x3) * 16;
    session.apply_movk(pc, rd, Expr::Const(imm << shift));
    true
}

pub fn apply_addsub_imm_encoding(session: &mut SymbolicSession, pc: u64, op: u32) -> bool {
    if classify_transfer(op) != TransferClass::AddSub || (op & (1 << 22)) != 0 {
        return false;
    }
    let rd = (op & 0x1f) as usize;
    let rn = ((op >> 5) & 0x1f) as usize;
    let imm = u64::from((op >> 10) & 0xfff);
    let value = if (op & (1 << 30)) != 0 {
        Expr::Sub(Box::new(Expr::Const(0)), Box::new(Expr::Const(imm)))
    } else {
        Expr::Const(imm)
    };
    if (op & (1 << 30)) != 0 {
        session.observe_sub(pc, rd, rn, value);
    } else {
        session.observe_add(pc, rd, rn, value);
    }
    true
}

pub fn apply_supported_encoding(session: &mut SymbolicSession, pc: u64, op: u32) -> bool {
    apply_movz_encoding(session, pc, op)
        || apply_movk_encoding(session, pc, op)
        || apply_addsub_imm_encoding(session, pc, op)
}

/// Applies unsigned-immediate AArch64 LDR/STR when the base address is concrete.
pub fn apply_load_store_encoding(
    session: &mut SymbolicSession,
    snapshot: &RegisterSnapshot,
) -> bool {
    let op = snapshot.opcode;
    if op & 0x3b00_0000 != 0x3900_0000 {
        return false;
    }
    let size = 1u8 << ((op >> 30) & 0x3);
    let load = (op & (1 << 22)) != 0;
    let rn = ((op >> 5) & 0x1f) as usize;
    let rt = (op & 0x1f) as usize;
    let offset = u64::from((op >> 10) & 0xfff) * u64::from(size);
    let address = snapshot.regs[rn].wrapping_add(offset);
    if load {
        session.observe_load(snapshot.pc, rt, address, size * 8);
    } else if let Some(expr) = session.state.regs[rt].clone() {
        session.observe_store(snapshot.pc, address, expr);
    } else {
        session.observe_store(
            snapshot.pc,
            address,
            Expr::Unknown {
                pc: snapshot.pc,
                width: size * 8,
            },
        );
    }
    true
}

/// Records the concrete outcome and symbolic condition of CBZ/CBNZ and B.cond.
pub fn apply_branch_encoding(session: &mut SymbolicSession, snapshot: &RegisterSnapshot) -> bool {
    let op = snapshot.opcode;
    if op & 0x7f00_0000 == 0x3400_0000 {
        let rt = (op & 0x1f) as usize;
        let imm = ((op >> 5) & 0x7ffff) as i64;
        let imm = (imm << 43) >> 41;
        let target = snapshot.pc.wrapping_add(imm as u64);
        let value = snapshot.regs[rt];
        let nonzero = (op & (1 << 24)) != 0;
        let taken = if nonzero { value != 0 } else { value == 0 };
        let condition = session.state.regs[rt]
            .as_ref()
            .map(|e| format!("{e:?}"))
            .unwrap_or_else(|| format!("x{rt}={value:#x}"));
        session.observe_branch(snapshot.pc, target, taken, condition);
        return true;
    }
    if op & 0xff00_0010 == 0x5400_0000 {
        let imm = ((op >> 5) & 0x7ffff) as i64;
        let imm = (imm << 43) >> 41;
        let target = snapshot.pc.wrapping_add(imm as u64);
        let cond = op & 0xf;
        let nzcv = (snapshot.pstate >> 28) & 0xf;
        let (n, z, c, v) = (nzcv & 8 != 0, nzcv & 4 != 0, nzcv & 2 != 0, nzcv & 1 != 0);
        let taken = match cond {
            0 => z,
            1 => !z,
            2 => c,
            3 => !c,
            4 => n,
            5 => !n,
            6 => v,
            7 => !v,
            8 => c && !z,
            9 => !c || z,
            10 => n == v,
            11 => n != v,
            12 => !z && n == v,
            13 => z || n != v,
            _ => true,
        };
        session.observe_branch(
            snapshot.pc,
            target,
            taken,
            format!("cond={cond:#x} pstate={:#x}", snapshot.pstate),
        );
        return true;
    }
    false
}

pub fn apply_compare_encoding(session: &mut SymbolicSession, snapshot: &RegisterSnapshot) -> bool {
    let op = snapshot.opcode;
    if op & 0x7f00_001f == 0x7100_001f {
        let rn = ((op >> 5) & 0x1f) as usize;
        let imm = (op >> 10) & 0xfff;
        let lhs = session.state.regs[rn]
            .clone()
            .unwrap_or(Expr::Const(snapshot.regs[rn]));
        session.observe_compare(snapshot.pc, format!("{lhs:?} - {imm:#x}"));
        return true;
    }
    if op & 0xffe0_fc1f == 0x6b00_001f || op & 0xffe0_fc1f == 0xeb00_001f {
        let rn = ((op >> 5) & 0x1f) as usize;
        let rm = ((op >> 16) & 0x1f) as usize;
        let lhs = session.state.regs[rn]
            .clone()
            .unwrap_or(Expr::Const(snapshot.regs[rn]));
        let rhs = session.state.regs[rm]
            .clone()
            .unwrap_or(Expr::Const(snapshot.regs[rm]));
        session.observe_compare(snapshot.pc, format!("{lhs:?} - {rhs:?}"));
        return true;
    }
    false
}

#[derive(Clone, Debug, Default)]
pub struct SymbolicState {
    pub regs: [Option<Expr>; 32],
    pub memory: std::collections::BTreeMap<u64, Expr>,
}

pub struct SymbolicSession {
    pub state: SymbolicState,
    pub pc_start: u64,
    pub pc_end: u64,
    pub branches: Vec<String>,
    pub last_compare: Option<String>,
}

impl SymbolicSession {
    pub fn new(pc_start: u64, pc_end: u64) -> Self {
        Self {
            state: SymbolicState::default(),
            pc_start,
            pc_end,
            branches: Vec::new(),
            last_compare: None,
        }
    }

    pub fn observes(&self, pc: u64) -> bool {
        (self.pc_start..=self.pc_end).contains(&pc)
    }

    pub fn observe_movz(&mut self, pc: u64, dst: usize, value: u64) {
        if self.observes(pc) {
            self.state.set(dst, Expr::Const(value));
        }
    }

    pub fn observe_input(&mut self, pc: u64, dst: usize, name: &'static str) {
        if self.observes(pc) {
            self.state.seed_input(dst, name);
        }
    }

    pub fn observe_add(&mut self, pc: u64, dst: usize, lhs: usize, rhs: Expr) {
        if self.observes(pc) {
            self.state.add(dst, lhs, rhs);
        }
    }

    pub fn observe_sub(&mut self, pc: u64, dst: usize, lhs: usize, rhs: Expr) {
        if self.observes(pc) {
            self.state.sub(dst, lhs, rhs);
        }
    }

    pub fn observe_store(&mut self, pc: u64, address: u64, expr: Expr) {
        if self.observes(pc) {
            self.state.store(address, expr);
        }
    }

    pub fn observe_load(&mut self, pc: u64, dst: usize, address: u64, width: u8) {
        if self.observes(pc) {
            let expr = self.state.load(address, pc, width);
            self.state.set(dst, expr);
        }
    }

    pub fn seed_sepos_inputs(&mut self) {
        self.state
            .store(0x1000_01018, Expr::Input("sepos.loader_context.magic"));
        self.state
            .store(0x1000_01020, Expr::Input("sepos.loader_context.version"));
        self.state
            .store(0x1000_01030, Expr::Input("sepos.loader_context.dram_base"));
        self.state
            .store(0x1000_01038, Expr::Input("sepos.loader_context.dram_size"));
        self.state.store(0x1030, Expr::Input("context.dram_base"));
        self.state.store(0x1038, Expr::Input("context.dram_size"));
        self.state
            .store(0x2412_00318, Expr::Input("mddr.config_0x318"));
    }

    pub fn snapshot(&self) -> String {
        let regs = self
            .state
            .regs
            .iter()
            .enumerate()
            .filter_map(|(index, value)| value.as_ref().map(|expr| format!("x{index}={expr:?}")))
            .collect::<Vec<_>>();
        format!(
            "pc_range={:#x}..={:#x} {}",
            self.pc_start,
            self.pc_end,
            regs.join(" ")
        )
    }

    pub fn apply_unknown(&mut self, pc: u64, dst: usize, width: u8) {
        if self.observes(pc) {
            self.state.set(dst, Expr::Unknown { pc, width });
        }
    }

    pub fn apply_movz(&mut self, pc: u64, dst: usize, value: u64) {
        self.observe_movz(pc, dst, value);
    }

    pub fn apply_movk(&mut self, pc: u64, dst: usize, value: Expr) {
        if self.observes(pc) {
            self.state.set(
                dst,
                Expr::Add(
                    Box::new(self.state.regs[dst].clone().unwrap_or(Expr::Const(0))),
                    Box::new(value),
                ),
            );
        }
    }

    pub fn observe_branch(&mut self, pc: u64, target: u64, taken: bool, condition: String) {
        if self.observes(pc) {
            let compare = self.last_compare.as_deref().unwrap_or("none");
            self.branches.push(format!("pc={pc:#x} target={target:#x} taken={taken} condition={condition} compare={compare}"));
        }
    }

    pub fn observe_compare(&mut self, pc: u64, expression: String) {
        if self.observes(pc) {
            self.last_compare = Some(expression);
        }
    }
}

impl SymbolicState {
    pub fn seed_input(&mut self, reg: usize, name: &'static str) {
        self.regs[reg] = Some(Expr::Input(name));
    }

    pub fn set(&mut self, reg: usize, expr: Expr) {
        self.regs[reg] = Some(expr);
    }

    pub fn add(&mut self, dst: usize, lhs: usize, rhs: Expr) {
        self.regs[dst] = Some(Expr::Add(
            Box::new(
                self.regs[lhs]
                    .clone()
                    .unwrap_or(Expr::Unknown { pc: 0, width: 64 }),
            ),
            Box::new(rhs),
        ));
    }

    pub fn sub(&mut self, dst: usize, lhs: usize, rhs: Expr) {
        self.regs[dst] = Some(Expr::Sub(
            Box::new(
                self.regs[lhs]
                    .clone()
                    .unwrap_or(Expr::Unknown { pc: 0, width: 64 }),
            ),
            Box::new(rhs),
        ));
    }

    pub fn shl(&mut self, dst: usize, src: usize, amount: u8) {
        self.regs[dst] = Some(Expr::Shl(
            Box::new(
                self.regs[src]
                    .clone()
                    .unwrap_or(Expr::Unknown { pc: 0, width: 64 }),
            ),
            amount,
        ));
    }

    pub fn ubfm(&mut self, dst: usize, src: usize, lsb: u8, msb: u8) {
        self.regs[dst] = Some(Expr::Ubfm {
            value: Box::new(
                self.regs[src]
                    .clone()
                    .unwrap_or(Expr::Unknown { pc: 0, width: 64 }),
            ),
            lsb,
            msb,
        });
    }

    pub fn store(&mut self, address: u64, expr: Expr) {
        self.memory.insert(address, expr);
    }

    pub fn load(&self, address: u64, pc: u64, width: u8) -> Expr {
        let _ = pc;
        self.memory
            .get(&address)
            .cloned()
            .unwrap_or(Expr::Load { address, width })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_addsub_imm_encoding, apply_compare_encoding, apply_movk_encoding,
        apply_movz_encoding, apply_supported_encoding, classify_transfer, Expr, RegisterSnapshot,
        SymbolicSession, SymbolicState, TransferClass,
    };

    #[test]
    fn seeds_and_replaces_register_provenance() {
        let mut state = SymbolicState::default();
        state.seed_input(8, "mddr_ctrl");
        assert_eq!(state.regs[8], Some(Expr::Input("mddr_ctrl")));
        state.set(8, Expr::Const(0x100));
        assert_eq!(state.regs[8], Some(Expr::Const(0x100)));
    }

    #[test]
    fn classifies_pinned_transfer_families() {
        assert_eq!(classify_transfer(0xd2800000), TransferClass::MovZ);
        assert_eq!(classify_transfer(0xf2800000), TransferClass::MovK);
        assert_eq!(classify_transfer(0x91000000), TransferClass::AddSub);
        assert_eq!(classify_transfer(0xd503201f), TransferClass::Unknown);
    }

    #[test]
    fn extracts_movz_operands() {
        let mut session = SymbolicSession::new(0x1000, 0x2000);
        assert!(apply_movz_encoding(&mut session, 0x1100, 0xd2800d48));
        assert_eq!(session.state.regs[8], Some(Expr::Const(0x6a)));
    }

    #[test]
    fn extracts_movk_operands() {
        let mut session = SymbolicSession::new(0x1000, 0x2000);
        assert!(apply_movz_encoding(&mut session, 0x1100, 0xd2800008));
        assert!(apply_movk_encoding(&mut session, 0x1104, 0xf2a00028));
        assert!(matches!(session.state.regs[8], Some(Expr::Add(_, _))));
    }

    #[test]
    fn extracts_add_sub_immediate_encodings() {
        let mut session = SymbolicSession::new(0x1000, 0x2000);
        session.observe_input(0x10ff, 1, "base");
        assert!(apply_addsub_imm_encoding(&mut session, 0x1100, 0x9100_0420));
        assert!(matches!(session.state.regs[0], Some(Expr::Add(_, _))));
        assert!(apply_addsub_imm_encoding(&mut session, 0x1104, 0xd100_0420));
        assert!(matches!(session.state.regs[0], Some(Expr::Sub(_, _))));
    }

    #[test]
    fn dispatches_supported_encodings_only() {
        let mut session = SymbolicSession::new(0x1000, 0x2000);
        assert!(apply_supported_encoding(&mut session, 0x1100, 0xd2800008));
        assert!(!apply_supported_encoding(&mut session, 0x1104, 0xd503201f));
    }

    #[test]
    fn records_register_compare_provenance() {
        let mut session = SymbolicSession::new(0x8000, 0x9000);
        session.state.set(8, Expr::Input("address_a"));
        session.state.set(9, Expr::Input("address_b"));
        let snapshot = RegisterSnapshot {
            pc: 0x8560,
            opcode: 0xeb09_011f,
            regs: [0; 31],
            sp: 0,
            pstate: 0,
            el: 1,
        };
        assert!(apply_compare_encoding(&mut session, &snapshot));
        assert_eq!(
            session.last_compare.as_deref(),
            Some("Input(\"address_a\") - Input(\"address_b\")")
        );
    }

    #[test]
    fn session_scopes_observations() {
        let session = super::SymbolicSession::new(0x8000, 0x8200);
        assert!(session.observes(0x8100));
        assert!(!session.observes(0x9000));
    }

    #[test]
    fn session_observes_scoped_values() {
        let mut session = super::SymbolicSession::new(0x8000, 0x8200);
        session.observe_input(0x8100, 8, "mddr");
        assert_eq!(session.state.regs[8], Some(Expr::Input("mddr")));
        session.observe_movz(0x9000, 9, 1);
        assert_eq!(session.state.regs[9], None);
        session.observe_add(0x8104, 10, 8, Expr::Const(0x400));
        assert!(matches!(session.state.regs[10], Some(Expr::Add(_, _))));
        session.observe_sub(0x8108, 11, 10, Expr::Const(1));
        assert!(matches!(session.state.regs[11], Some(Expr::Sub(_, _))));
        session.observe_store(0x810c, 0x1030, Expr::Input("dram_base"));
        session.observe_load(0x8110, 12, 0x1030, 64);
        assert_eq!(session.state.regs[12], Some(Expr::Input("dram_base")));
        session.seed_sepos_inputs();
        assert_eq!(
            session.state.load(0x1038, 0x8110, 64),
            Expr::Input("context.dram_size")
        );
        assert!(session.snapshot().contains("pc_range=0x8000"));
    }

    #[test]
    fn preserves_arithmetic_provenance() {
        let mut state = SymbolicState::default();
        state.seed_input(8, "mddr_ctrl");
        state.add(9, 8, Expr::Const(0x400));
        assert!(matches!(state.regs[9], Some(Expr::Add(_, _))));
        state.sub(10, 9, Expr::Const(1));
        state.shl(11, 10, 10);
        state.ubfm(12, 11, 1, 63);
        assert!(matches!(state.regs[12], Some(Expr::Ubfm { .. })));
        state.store(0x1030, Expr::Input("context.dram_base"));
        assert_eq!(
            state.load(0x1030, 0x8100, 64),
            Expr::Input("context.dram_base")
        );
        assert!(matches!(state.load(0x1038, 0x8104, 64), Expr::Load { .. }));
    }
}
