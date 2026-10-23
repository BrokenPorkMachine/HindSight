//! HindSight built-in deterministic ARM64 research target.
//!
//! This is intentionally a narrow execution laboratory: it contains the
//! iPhemeral CPU/MMU/device/debugger core but not restore, jailbreak, TSS,
//! device-secret, firmware-key, SSH, or network automation machinery.

use iphem_core::{BusRegion, Machine, SparseRam};
use iphem_soc::{build_with_rom, config, SoCId};
use std::fs;
use std::time::Duration;
use vg_emu_server::debug::{DebugAction, DebugControlPlane, DebugServerConfig};

fn parse_soc(value: &str) -> Result<SoCId, String> {
    match value.to_ascii_lowercase().as_str() {
        "a7" | "s5l8960x" | "s5l8960xi" => Ok(SoCId::A7),
        "a8" | "t7000" => Ok(SoCId::A8),
        "a9" | "s8000" => Ok(SoCId::S8000),
        "t8010" => Ok(SoCId::T8010),
        "t8015" => Ok(SoCId::T8015),
        "t8020" => Ok(SoCId::T8020),
        "t8027" => Ok(SoCId::T8027),
        "t8030" => Ok(SoCId::T8030),
        "t8101" => Ok(SoCId::T8101),
        _ => Err(format!("unsupported SoC: {value}")),
    }
}

fn build_target(soc: SoCId, rom: &[u8]) -> Result<Machine, String> {
    let mut machine = build_with_rom(soc, rom, true);
    let cfg = config(soc);

    // Some iPhemeral builders already map SRAM. If they do not, add a bounded
    // sparse region using the SoC descriptor rather than inventing a fixed map.
    if cfg.sram_size > 0 {
        // An overlap means the SoC builder already supplied the window, which is fine.
        let _ = machine.bus.map(BusRegion {
            base: cfg.sram_base,
            size: cfg.sram_size,
            device: Box::new(SparseRam::new(cfg.sram_base, cfg.sram_size)),
        });
    }

    let state = &mut machine.cores.active_mut().state;
    state.pc = cfg.rom_base;
    let stack = cfg.sram_base.saturating_add(cfg.sram_size).saturating_sub(0x400);
    state.sp = stack;
    state.sp_el[1] = stack;
    state.sp_el[3] = stack;
    state.el = soc.reset_entry_level();
    state.currentel = state.el as u8;
    if state.currentel == 3 {
        state.pstate = 0x3cd;
        state.elr_el3 = state.pc;
        state.spsr_el3 = state.pstate;
        state.sp = state.sp_el[3];
    } else {
        state.pstate = 0x3c5;
    }
    state.daif = 0xf;
    state.spsel = true;
    Ok(machine)
}

fn help() {
    println!("HindSight VectorEmu 0.5.13\n\nUsage:\n  vg-emu-server --rom PATH [--soc t8020] [--port 37920]\n\nThe server binds only to 127.0.0.1 and starts paused. It accepts the\nnewline-delimited JSON debugger protocol documented in docs/VECTOREMU_PROTOCOL.md.");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let mut rom: Option<String> = None;
    let mut soc = SoCId::T8020;
    let mut port: u16 = 37920;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--rom" if index + 1 < args.len() => { rom = Some(args[index + 1].clone()); index += 2; }
            "--soc" if index + 1 < args.len() => { soc = parse_soc(&args[index + 1])?; index += 2; }
            "--port" if index + 1 < args.len() => { port = args[index + 1].parse()?; index += 2; }
            "--version" => { println!("vg-emu-server 0.5.13"); return Ok(()); }
            "--help" | "-h" => { help(); return Ok(()); }
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }

    let path = rom.ok_or("--rom is required")?;
    let size = fs::metadata(&path)?.len();
    if size == 0 || size > 64 * 1024 * 1024 { return Err("ROM size must be 1 byte to 64 MiB".into()); }
    let rom_bytes = fs::read(&path)?;
    let mut machine = build_target(soc, &rom_bytes).map_err(|e| format!("target setup failed: {e}"))?;
    let mut debugger = DebugControlPlane::bind(DebugServerConfig::standalone(
        port,
        format!("VectorEmu {soc:?} • {}", std::path::Path::new(&path).file_name().unwrap_or_default().to_string_lossy()),
    ))?;
    println!("VectorEmu listening on {} for {:?}", debugger.local_addr()?, soc);

    let mut shutdown = false;
    while !shutdown {
        for action in debugger.poll(&mut machine)? {
            match action {
                DebugAction::Reset => {
                    machine = build_target(soc, &rom_bytes).map_err(|e| format!("reset failed: {e}"))?;
                    debugger.reset_completed();
                }
                DebugAction::Shutdown => shutdown = true,
            }
        }
        if debugger.is_running() {
            for _ in 0..1024 {
                if debugger.step_target(&mut machine).is_none() { break; }
            }
        } else {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    Ok(())
}
