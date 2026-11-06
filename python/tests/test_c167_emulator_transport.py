from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


def source(path: str) -> str:
    return (ROOT / path).read_text()


def test_c167_emulator_is_admitted_only_for_captured_ecu_artifacts():
    coordinator = source("PlatformServices/CoordinatorService/main.swift")
    assert 'source.options?.format == "ecu" || source.options?.architecture == "c167"' in coordinator
    assert "VerifiedPageCache.stamp(source.path)" in coordinator
    assert "Captured ECU changed during bounded execution." in coordinator


def test_c167_emulator_result_is_identity_and_bounds_validated_at_both_boundaries():
    messages = source("Shared/WorkbenchMessages.swift")
    supervisor = source("PlatformServices/CoordinatorService/WorkerSupervisor.swift")
    model = source("App/WorkbenchModel.swift")
    assert "func validated(requestID: String) -> C167EmulationResult?" in messages
    assert "decoded.validated(requestID: id)" in supervisor
    assert "reply.result?.validated(requestID: requestID)" in model


def test_c167_emulator_has_a_dedicated_ecu_workspace_and_bounded_ui():
    catalog = source("App/WorkspaceCatalog.swift")
    content = source("App/ContentView.swift")
    assert '"ECU Emulator"' in catalog
    assert "private func ecuEmulatorWorkspace()" in content
    assert "No peripherals, vehicle buses, or flashing are modeled." in content
    assert "bounded CP-selected register banks" in content
    assert ".frame(maxWidth: 190)" in content


def test_c167_emulator_exposes_dpp_watch_state_and_replay_controls():
    messages = source("Shared/WorkbenchMessages.swift")
    model = source("App/WorkbenchModel.swift")
    content = source("App/ContentView.swift")
    worker = source("rust/crates/worker/src/binary.rs")
    assert "let dpp: [String: UInt16]" in messages
    assert "dpp.count == 4" in messages
    assert '\\\"dpp\\\":{{{dpp}}}' in worker
    assert "func stepC167Emulator()" in model
    assert "func continueC167Emulator()" in model
    assert "func resetC167Emulator()" in model
    for label in ('Button("Step"', 'Button("Continue"', 'Button("Reset"', 'DPP\\($0)'):
        assert label in content


def test_c167_breakpoint_is_carried_to_the_fail_closed_worker():
    protocol = source("Shared/CoordinatorProtocol.swift")
    supervisor = source("PlatformServices/CoordinatorService/WorkerSupervisor.swift")
    worker = source("rust/crates/worker/src/c167_emu.rs")
    content = source("App/ContentView.swift")
    assert "breakpoint: String" in protocol
    assert '"--emulate-c167", path, entry, steps, stack, breakpoint, watchpoint, ram, id' in supervisor
    assert "if breakpoint == Some(self.pc)" in worker
    assert 'TextField("Breakpoint"' in content


def test_c167_physical_memory_watchpoint_is_end_to_end():
    protocol = source("Shared/CoordinatorProtocol.swift")
    supervisor = source("PlatformServices/CoordinatorService/WorkerSupervisor.swift")
    worker = source("rust/crates/worker/src/c167_emu.rs")
    content = source("App/ContentView.swift")
    assert "watchpoint: String" in protocol
    assert 'breakpoint, watchpoint, ram, id' in supervisor
    assert "Stop::Watchpoint { address, access }" in worker
    assert "self.record_access" in worker
    assert 'TextField("Physical watch"' in content


def test_c167_ram_seeds_are_bounded_separate_and_visible_end_to_end():
    protocol = source("Shared/CoordinatorProtocol.swift")
    supervisor = source("PlatformServices/CoordinatorService/WorkerSupervisor.swift")
    worker = source("rust/crates/worker/src/c167_emu.rs")
    messages = source("Shared/WorkbenchMessages.swift")
    content = source("App/ContentView.swift")
    assert "ram: String" in protocol
    assert "breakpoint, watchpoint, ram, id" in supervisor
    assert "ram: BTreeMap<u32, u8>" in worker
    assert "pub fn seed_ram" in worker
    assert "ram_seed_bytes <= 4096" in messages and "ram.count <= 256" in messages
    assert 'TextField("RAM seeds (physical=hex bytes, comma-separated)"' in content
    assert 'Text("RAM preview")' in content


def test_c167_cp_sp_and_md_accumulator_are_validated_and_visible():
    messages = source("Shared/WorkbenchMessages.swift")
    worker = source("rust/crates/worker/src/c167_emu.rs")
    content = source("App/ContentView.swift")
    for field in ("let cp: UInt16", "let mdh: UInt16", "let mdl: UInt16"):
        assert field in messages
    assert "cp & 0x03ff == 0" in messages
    assert "raw[1] == 8" in worker and "raw[1] == 9" in worker
    assert "self.mdh = value" in worker and "self.mdl = value" in worker
    assert "register_banks: [[u16; 16]; 64]" in worker
    assert "fn set_cp(&mut self, value: u16)" in worker
    assert 'Text(String(format: "CP  0x%04X"' in content
    assert 'Text(String(format: "MD  0x%04X:%04X"' in content


def test_c167_md_arithmetic_is_bounded_and_faults_before_mutation():
    worker = source("rust/crates/worker/src/c167_emu.rs")
    fixture = source("scripts/test-binary-formats.py")
    assert "fn multiply(&mut self, signed: bool" in worker
    assert "fn divide(&mut self, operation: u8" in worker
    assert 'reason: "division by zero"' in worker
    assert 'reason: "division quotient overflow"' in worker
    assert "c167-div-zero" in fixture
