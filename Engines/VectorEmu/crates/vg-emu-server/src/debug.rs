//! Loopback debugger control plane shared by the live emulator and standalone target.

use iphem_arm::{disassemble, step_with_device_tick, ExecError};
use iphem_core::{CpuRunState, Machine};
use serde_json::{json, Value};
use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_LINE: usize = 1 << 20;
const MAX_TRACE: usize = 1024;

#[derive(Clone, Debug)]
pub struct DebugServerConfig {
    pub port: u16,
    pub target_name: String,
    /// Whether guest execution proceeds when no debugger owns the target.
    pub run_when_detached: bool,
    /// Initial execution state. Standalone targets start paused; live targets run.
    pub initially_running: bool,
    /// Standalone firmware analysis stops on WFI. A live OS must keep ticking devices.
    pub stop_on_wait: bool,
}

impl DebugServerConfig {
    pub fn live(port: u16, target_name: impl Into<String>) -> Self {
        Self {
            port,
            target_name: target_name.into(),
            run_when_detached: true,
            initially_running: true,
            stop_on_wait: false,
        }
    }

    pub fn standalone(port: u16, target_name: impl Into<String>) -> Self {
        Self {
            port,
            target_name: target_name.into(),
            run_when_detached: false,
            initially_running: false,
            stop_on_wait: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugAction {
    Reset,
    Shutdown,
}

struct Client {
    stream: TcpStream,
    input: Vec<u8>,
    output: Vec<u8>,
}

impl Client {
    fn new(stream: TcpStream) -> std::io::Result<Self> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        Ok(Self {
            stream,
            input: Vec::new(),
            output: Vec::new(),
        })
    }
}

/// A single-controller debug service. The service never owns or shares a
/// `Machine`; callers lend it the machine at safe instruction boundaries.
pub struct DebugControlPlane {
    listener: Option<TcpListener>,
    client: Option<Client>,
    target_name: String,
    run_when_detached: bool,
    stop_on_wait: bool,
    attached: bool,
    running: bool,
    remaining: Option<u64>,
    breakpoints: BTreeSet<u64>,
    trace: VecDeque<Value>,
    reason: String,
    actions: VecDeque<DebugAction>,
}

impl DebugControlPlane {
    pub fn bind(config: DebugServerConfig) -> std::io::Result<Self> {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, config.port))?;
        listener.set_nonblocking(true)?;
        Ok(Self::new(Some(listener), config))
    }

    fn new(listener: Option<TcpListener>, config: DebugServerConfig) -> Self {
        Self {
            listener,
            client: None,
            target_name: config.target_name,
            run_when_detached: config.run_when_detached,
            stop_on_wait: config.stop_on_wait,
            attached: false,
            running: config.initially_running,
            remaining: None,
            breakpoints: BTreeSet::new(),
            trace: VecDeque::new(),
            reason: if config.initially_running {
                "running".into()
            } else {
                "reset".into()
            },
            actions: VecDeque::new(),
        }
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener
            .as_ref()
            .ok_or_else(|| std::io::Error::other("debug listener is unavailable"))?
            .local_addr()
    }

    pub fn is_attached(&self) -> bool {
        self.attached
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Accept requests and execute them against `machine`. This method is
    /// nonblocking and should be called regularly from the emulator thread.
    pub fn poll(&mut self, machine: &mut Machine) -> std::io::Result<Vec<DebugAction>> {
        while let Some(listener) = self.listener.as_ref() {
            match listener.accept() {
                Ok((stream, _)) => {
                    // Short-lived CLI clients close after each reply. Reap a
                    // closed controller before deciding that the target is busy.
                    let previous_closed = self.client.as_ref().is_some_and(|client| {
                        let mut probe = [0u8; 1];
                        matches!(client.stream.peek(&mut probe), Ok(0))
                    });
                    if previous_closed {
                        self.client = None;
                    }
                    if self.client.is_none() {
                        self.client = Some(Client::new(stream)?);
                    } else {
                        let mut stream = stream;
                        let response = json!({
                            "id": null,
                            "ok": false,
                            "error": "another debugger controls this target"
                        });
                        let mut bytes = serde_json::to_vec(&response)?;
                        bytes.push(b'\n');
                        let _ = stream.write_all(&bytes);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            }
        }

        if let Some(mut client) = self.client.take() {
            let mut connected = true;
            let mut buf = [0u8; 8192];
            loop {
                match client.stream.read(&mut buf) {
                    Ok(0) => {
                        connected = false;
                        break;
                    }
                    Ok(count) => {
                        client.input.extend_from_slice(&buf[..count]);
                        if client.input.len() > MAX_LINE {
                            let response = json!({
                                "id": null,
                                "ok": false,
                                "error": "request exceeds 1 MiB"
                            });
                            client.output.extend(serde_json::to_vec(&response)?);
                            client.output.push(b'\n');
                            connected = false;
                            break;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error) => return Err(error),
                }
            }

            while let Some(end) = client.input.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = client.input.drain(..=end).collect();
                let response = match serde_json::from_slice::<Value>(&line) {
                    Ok(request) => self.reply(machine, &request),
                    Err(error) => json!({"id":null,"ok":false,"error":error.to_string()}),
                };
                client.output.extend(serde_json::to_vec(&response)?);
                client.output.push(b'\n');
            }

            while !client.output.is_empty() {
                match client.stream.write(&client.output) {
                    Ok(0) => {
                        connected = false;
                        break;
                    }
                    Ok(count) => {
                        client.output.drain(..count);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
                        ) =>
                    {
                        connected = false;
                        break;
                    }
                    Err(error) => return Err(error),
                }
            }
            if connected {
                self.client = Some(client);
            }
        }

        Ok(self.actions.drain(..).collect())
    }

    /// Execute one host-scheduled instruction when debugger state permits it.
    /// `None` means the target is paused or stopped at a breakpoint.
    pub fn step_target(&mut self, machine: &mut Machine) -> Option<Result<(), ExecError>> {
        if !self.execution_permitted(machine) {
            return None;
        }
        Some(self.execute_one(machine, true))
    }

    /// Notify the service after the host has rebuilt the live machine.
    pub fn reset_completed(&mut self) {
        self.running = !self.attached && self.run_when_detached;
        self.remaining = None;
        self.trace.clear();
        self.reason = "reset".into();
    }

    fn execution_permitted(&mut self, machine: &Machine) -> bool {
        if !self.running {
            return false;
        }
        let pc = machine.cores.active().state.pc;
        if self.attached && self.breakpoints.contains(&pc) {
            self.running = false;
            self.remaining = None;
            self.reason = "breakpoint".into();
            return false;
        }
        if machine.cores.active().halted() {
            self.running = false;
            self.remaining = None;
            self.reason = "halted".into();
            return false;
        }
        true
    }

    fn execute_one(
        &mut self,
        machine: &mut Machine,
        count_run_budget: bool,
    ) -> Result<(), ExecError> {
        if self.attached {
            let pc = machine.cores.active().state.pc;
            if let Ok(word) = machine.mem_read(pc, 4) {
                let word = word as u32;
                if self.trace.len() == MAX_TRACE {
                    self.trace.pop_front();
                }
                self.trace.push_back(json!({
                    "pc": hx(pc),
                    "word": format!("{word:08x}"),
                    "text": disassemble(pc, word)
                }));
            }
        }
        let result = step_with_device_tick(machine);
        machine.step_counter = machine.step_counter.saturating_add(1);
        if let Err(error) = &result {
            self.running = false;
            self.remaining = None;
            self.reason = format!("execution fault: {error:?}");
            return result;
        }
        if self.stop_on_wait
            && matches!(
                machine.cores.active().run_state,
                CpuRunState::WaitingForInterrupt
            )
        {
            self.running = false;
            self.remaining = None;
            self.reason = "waiting for interrupt".into();
        } else if count_run_budget {
            if let Some(remaining) = self.remaining.as_mut() {
                *remaining = remaining.saturating_sub(1);
                if *remaining == 0 {
                    self.running = false;
                    self.remaining = None;
                    self.reason = "step limit".into();
                }
            }
        }
        result
    }

    fn reply(&mut self, machine: &mut Machine, request: &Value) -> Value {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        match self.command(machine, request) {
            Ok(result) => json!({"id":id,"ok":true,"result":result}),
            Err(error) => json!({"id":id,"ok":false,"error":error}),
        }
    }

    fn command(&mut self, machine: &mut Machine, request: &Value) -> Result<Value, String> {
        let command = request
            .get("command")
            .and_then(Value::as_str)
            .ok_or("missing command")?;
        match command {
            "hello" => {
                return Ok(json!({
                    "protocol":"iphem-re/2",
                    "architecture":"aarch64",
                    "target":self.target_name,
                    "live":self.run_when_detached,
                    "features":["run","pause","step","reset","registers","cpu_state","memory","regions","breakpoints","watch_write","trace","disassemble","assemble","header","convert"]
                }));
            }
            "attach" => {
                self.attached = true;
                return Ok(self.status(machine));
            }
            "detach" => {
                self.attached = false;
                self.running = self.run_when_detached;
                self.remaining = None;
                self.reason = "detached".into();
                return Ok(self.status(machine));
            }
            "shutdown" => {
                self.running = false;
                self.actions.push_back(DebugAction::Shutdown);
                self.reason = "shutdown requested".into();
                return Ok(json!({"shutdown":true}));
            }
            "status" => return Ok(self.status(machine)),
            _ => {}
        }
        if !self.attached {
            return Err("attach first".into());
        }
        match command {
            "reset" => {
                self.running = false;
                self.remaining = None;
                self.reason = "reset requested".into();
                self.actions.push_back(DebugAction::Reset);
                Ok(json!({"reset_requested":true}))
            }
            "pause" => {
                self.running = false;
                self.remaining = None;
                self.reason = "paused".into();
                Ok(self.status(machine))
            }
            "run" => {
                let steps = request
                    .get("steps")
                    .map(|_| num(request, "steps"))
                    .transpose()?
                    .unwrap_or(1_000_000);
                if steps == 0 || steps > 1_000_000_000 {
                    return Err("steps must be 1..=1000000000".into());
                }
                self.remaining = Some(steps);
                self.running = true;
                self.reason = "running".into();
                Ok(self.status(machine))
            }
            "step" => {
                self.paused()?;
                let count = request
                    .get("count")
                    .map(|_| num(request, "count"))
                    .transpose()?
                    .unwrap_or(1);
                if count == 0 || count > 10_000 {
                    return Err("count must be 1..=10000".into());
                }
                for _ in 0..count {
                    self.execute_one(machine, false)
                        .map_err(|error| format!("{error:?}"))?;
                    if self.stop_on_wait
                        && matches!(
                            machine.cores.active().run_state,
                            CpuRunState::WaitingForInterrupt
                        )
                    {
                        break;
                    }
                }
                self.running = false;
                self.remaining = None;
                if self.reason != "waiting for interrupt" {
                    self.reason = "step complete".into();
                }
                Ok(self.status(machine))
            }
            "registers" => Ok(registers(machine)),
            "cpu_state" => Ok(json!({
                "active_core":machine.cores.active_index(),
                "core_count":machine.cores.len(),
                "state":machine.cores.active().state,
                "run_state":machine.cores.active().run_state,
                "pending_irq":machine.cores.active().pending_irq,
                "pending_fiq":machine.cores.active().pending_fiq
            })),
            "set_register" => {
                self.paused()?;
                let name = request
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or("missing name")?;
                let value = num(request, "value")?;
                let state = &mut machine.cores.active_mut().state;
                match name {
                    "pc" => state.pc = value,
                    "sp" => state.sp = value,
                    _ if name.starts_with('x') => {
                        let index = name[1..].parse::<usize>().map_err(|_| "invalid register")?;
                        if index >= 31 {
                            return Err("invalid register".into());
                        }
                        state.x[index] = value;
                    }
                    _ => return Err("unsupported register".into()),
                }
                Ok(self.status(machine))
            }
            "regions" => Ok(json!({
                "regions":machine.bus.regions.iter().map(|region|json!({
                    "base":hx(region.base),
                    "size":hx(region.size),
                    "device":region.device.name()
                })).collect::<Vec<_>>()
            })),
            "read_memory" => {
                self.paused()?;
                let address = num(request, "address")?;
                let length = bounded(request, "length", 65_536)?;
                let bytes = machine
                    .mem_read_bytes(address, length)
                    .map_err(|error| error.to_string())?;
                Ok(json!({"address":hx(address),"data":encode_hex(&bytes)}))
            }
            "write_memory" => {
                self.paused()?;
                let address = num(request, "address")?;
                let encoded = request
                    .get("data")
                    .and_then(Value::as_str)
                    .ok_or("missing data")?;
                if encoded.len() > 131_072 {
                    return Err("write too large".into());
                }
                let bytes = decode_hex(encoded)?;
                if bytes.is_empty() {
                    return Err("empty write".into());
                }
                machine
                    .mem_write_bytes(address, &bytes)
                    .map_err(|error| error.to_string())?;
                Ok(json!({"written":bytes.len()}))
            }
            "disassemble" => {
                self.paused()?;
                let address = num(request, "address")?;
                let count = bounded(request, "count", 1024)?;
                let mut instructions = Vec::with_capacity(count);
                for index in 0..count {
                    let pc = address
                        .checked_add(index as u64 * 4)
                        .ok_or("address overflow")?;
                    let word = machine.mem_read(pc, 4).map_err(|error| error.to_string())? as u32;
                    instructions.push(json!({
                        "pc":hx(pc),
                        "word":format!("{word:08x}"),
                        "text":disassemble(pc, word)
                    }));
                }
                Ok(json!({"instructions":instructions}))
            }
            "assemble" => {
                self.paused()?;
                let source = request
                    .get("source")
                    .and_then(Value::as_str)
                    .ok_or("missing source")?;
                let bytes = assemble(source)?;
                Ok(json!({"data":encode_hex(&bytes),"length":bytes.len()}))
            }
            "breakpoint_add" | "breakpoint_remove" => {
                let address = num(request, "address")?;
                if command == "breakpoint_add" {
                    self.breakpoints.insert(address);
                } else {
                    self.breakpoints.remove(&address);
                }
                Ok(self.breakpoint_list())
            }
            "breakpoints" => Ok(self.breakpoint_list()),
            "trace" => {
                let count = request
                    .get("count")
                    .map(|_| bounded(request, "count", MAX_TRACE as u64))
                    .transpose()?
                    .unwrap_or(64);
                Ok(json!({
                    "events":self.trace.iter().rev().take(count).rev().cloned().collect::<Vec<_>>()
                }))
            }
            "watch_write" => {
                self.paused()?;
                let address = num(request, "address")?;
                let length = bounded(request, "length", 65_536)?;
                machine.write_watch_va = Some((address, length as u64));
                machine.write_watch_log.clear();
                Ok(json!({"address":hx(address),"length":length}))
            }
            "watch_clear" => {
                machine.write_watch_va = None;
                machine.write_watch_log.clear();
                Ok(json!({"cleared":true}))
            }
            "watch_log" => Ok(json!({
                "events":machine.write_watch_log.iter().rev().take(MAX_TRACE)
                    .map(|(step,pc,address,size,value)|json!({
                        "step":step,"pc":hx(*pc),"address":hx(*address),
                        "size":size,"value":hx(*value)
                    })).collect::<Vec<_>>()
            })),
            "header" => {
                let language = request
                    .get("language")
                    .and_then(Value::as_str)
                    .unwrap_or("c");
                Ok(json!({"language":language,"text":header(machine, language)?}))
            }
            "convert" => convert(request),
            _ => Err(format!("unknown command: {command}")),
        }
    }

    fn paused(&self) -> Result<(), String> {
        if self.running {
            Err("pause target first".into())
        } else {
            Ok(())
        }
    }

    fn status(&self, machine: &Machine) -> Value {
        json!({
            "attached":self.attached,
            "running":self.running,
            "reason":self.reason,
            "target":self.target_name,
            "pc":hx(machine.cores.active().state.pc),
            "steps":machine.step_counter,
            "remaining":self.remaining,
            "active_core":machine.cores.active_index(),
            "core_count":machine.cores.len(),
            "run_state":format!("{:?}",machine.cores.active().run_state)
        })
    }

    fn breakpoint_list(&self) -> Value {
        json!({
            "breakpoints":self.breakpoints.iter().map(|address|hx(*address)).collect::<Vec<_>>()
        })
    }
}

fn registers(machine: &Machine) -> Value {
    let state = &machine.cores.active().state;
    let mut registers = serde_json::Map::new();
    for (index, value) in state.x.iter().enumerate() {
        registers.insert(format!("x{index}"), json!(hx(*value)));
    }
    for (name, value) in [
        ("pc", state.pc),
        ("sp", state.sp),
        ("pstate", state.pstate),
        ("ttbr0_el1", state.ttbr0_el1),
        ("ttbr1_el1", state.ttbr1_el1),
        ("tcr_el1", state.tcr_el1),
        ("sctlr_el1", state.sctlr_el1),
        ("vbar_el1", state.vbar_el1),
        ("esr_el1", state.esr_el1),
        ("far_el1", state.far_el1),
        ("elr_el1", state.elr_el1),
    ] {
        registers.insert(name.into(), json!(hx(value)));
    }
    json!({
        "registers":registers,
        "el":state.el as u8,
        "daif":state.daif
    })
}

fn header(machine: &Machine, language: &str) -> Result<String, String> {
    if !matches!(language, "c" | "objc") {
        return Err("language must be c or objc".into());
    }
    let mut text = String::from(
        "/* Generated from active iPhemeral SoC physical map. */\n#pragma once\n#include <stdint.h>\n",
    );
    if language == "objc" {
        text.push_str("#ifdef __OBJC__\n#import <Foundation/Foundation.h>\n#endif\n");
    }
    text.push_str(
        "typedef struct { uint64_t base, size; const char *name; } iphem_region_t;\nstatic const iphem_region_t iphem_regions[] = {\n",
    );
    for region in &machine.bus.regions {
        text.push_str(&format!(
            "  {{ UINT64_C(0x{:x}), UINT64_C(0x{:x}), \"{}\" }},\n",
            region.base,
            region.size,
            region.device.name()
        ));
    }
    text.push_str(
        "};\n#define IPHEM_REGION_COUNT (sizeof(iphem_regions)/sizeof(iphem_regions[0]))\n",
    );
    Ok(text)
}

fn num(value: &Value, key: &str) -> Result<u64, String> {
    let number = value.get(key).ok_or_else(|| format!("missing {key}"))?;
    if let Some(number) = number.as_u64() {
        return Ok(number);
    }
    let text = number.as_str().ok_or_else(|| format!("invalid {key}"))?;
    if let Some(hex) = text.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).map_err(|error| error.to_string())
    } else {
        text.parse::<u64>().map_err(|error| error.to_string())
    }
}

fn bounded(value: &Value, key: &str, max: u64) -> Result<usize, String> {
    let number = num(value, key)?;
    if number == 0 || number > max {
        Err(format!("{key} must be 1..={max}"))
    } else {
        Ok(number as usize)
    }
}

fn hx(value: u64) -> String {
    format!("0x{value:016x}")
}

fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for &byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
    let bytes = text.as_bytes();
    if bytes.len() % 2 != 0 {
        return Err("hex data must contain an even number of digits".into());
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let hi = hex_nibble(pair[0]).ok_or_else(|| "invalid hexadecimal digit".to_string())?;
        let lo = hex_nibble(pair[1]).ok_or_else(|| "invalid hexadecimal digit".to_string())?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn convert(value: &Value) -> Result<Value, String> {
    let bytes = decode_hex(
        value
            .get("data")
            .and_then(Value::as_str)
            .ok_or("missing data")?,
    )?;
    let width = bytes.len();
    if !matches!(width, 1 | 2 | 4 | 8) {
        return Err("data must be 1, 2, 4, or 8 bytes".into());
    }
    let mut little = [0u8; 8];
    little[..width].copy_from_slice(&bytes);
    let mut big = [0u8; 8];
    big[8 - width..].copy_from_slice(&bytes);
    let little = u64::from_le_bytes(little);
    let big = u64::from_be_bytes(big);
    let signed_little = ((little << (64 - width * 8)) as i64) >> (64 - width * 8);
    let signed_big = ((big << (64 - width * 8)) as i64) >> (64 - width * 8);
    let float_little = match width {
        4 => json!(f32::from_bits(little as u32)),
        8 => json!(f64::from_bits(little)),
        _ => Value::Null,
    };
    let float_big = match width {
        4 => json!(f32::from_bits(big as u32)),
        8 => json!(f64::from_bits(big)),
        _ => Value::Null,
    };
    Ok(json!({
        "width":width,
        "unsigned_le":little.to_string(),
        "unsigned_be":big.to_string(),
        "signed_le":signed_little.to_string(),
        "signed_be":signed_big.to_string(),
        "float_le":float_little,
        "float_be":float_big,
        "pointer_le":hx(little),
        "bits_le":format!("{little:0width$b}",width=width*8),
        "ascii":bytes.iter().map(|byte|if byte.is_ascii_graphic(){*byte as char}else{'.'}).collect::<String>()
    }))
}

fn assemble(source: &str) -> Result<Vec<u8>, String> {
    if source.is_empty() || source.len() > 16_384 {
        return Err("source length must be 1..=16384".into());
    }
    let lower = source.to_ascii_lowercase();
    if lower.contains(".include") || lower.contains(".incbin") {
        return Err("file inclusion directives are not accepted".into());
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let output = std::env::temp_dir().join(format!("iphem-re-{}-{stamp}.o", std::process::id()));
    let result = (|| {
        let mut child = Command::new("clang")
            .args([
                "-target",
                "aarch64-none-elf",
                "-c",
                "-x",
                "assembler",
                "-",
                "-o",
            ])
            .arg(&output)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| error.to_string())?;
        child
            .stdin
            .take()
            .ok_or("missing clang stdin")?
            .write_all(source.as_bytes())
            .map_err(|error| error.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if child
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_some()
            {
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err("assembler timed out".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let completed = child
            .wait_with_output()
            .map_err(|error| error.to_string())?;
        if !completed.status.success() {
            return Err(String::from_utf8_lossy(&completed.stderr)
                .chars()
                .take(2048)
                .collect());
        }
        let object = fs::read(&output).map_err(|error| error.to_string())?;
        let code = elf_text(&object)?;
        if code.is_empty() || code.len() > 16_384 {
            return Err("assembled text length out of range".into());
        }
        Ok(code.to_vec())
    })();
    let _ = fs::remove_file(output);
    result
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, String> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset + 2)
            .ok_or("truncated ELF")?
            .try_into()
            .map_err(|_| "truncated ELF")?,
    ))
}

fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, String> {
    Ok(u64::from_le_bytes(
        bytes
            .get(offset..offset + 8)
            .ok_or("truncated ELF")?
            .try_into()
            .map_err(|_| "truncated ELF")?,
    ))
}

fn elf_text(bytes: &[u8]) -> Result<&[u8], String> {
    if bytes.get(..6) != Some(b"\x7fELF\x02\x01") || u16_at(bytes, 18)? != 183 {
        return Err("expected AArch64 little-endian ELF".into());
    }
    let base = u64_at(bytes, 40)? as usize;
    let stride = u16_at(bytes, 58)? as usize;
    let count = u16_at(bytes, 60)? as usize;
    let names = u16_at(bytes, 62)? as usize;
    if stride < 64 || count > 1024 || names >= count {
        return Err("invalid ELF section table".into());
    }
    let section = |index: usize| -> Result<&[u8], String> {
        let at = base
            .checked_add(index.checked_mul(stride).ok_or("ELF overflow")?)
            .ok_or("ELF overflow")?;
        bytes
            .get(at..at.checked_add(stride).ok_or("ELF overflow")?)
            .ok_or_else(|| "truncated ELF".into())
    };
    let name_section = section(names)?;
    let name_start = u64_at(name_section, 24)? as usize;
    let name_end = name_start
        .checked_add(u64_at(name_section, 32)? as usize)
        .ok_or("ELF overflow")?;
    let strings = bytes
        .get(name_start..name_end)
        .ok_or("truncated ELF names")?;
    for index in 0..count {
        let current = section(index)?;
        let kind = u32::from_le_bytes(current[4..8].try_into().map_err(|_| "bad ELF section")?);
        if kind == 4 || kind == 9 {
            return Err("assembly contains unresolved relocations".into());
        }
    }
    for index in 0..count {
        let current = section(index)?;
        let name =
            u32::from_le_bytes(current[..4].try_into().map_err(|_| "bad ELF name")?) as usize;
        if strings.get(name..name + 5) == Some(b".text") {
            let start = u64_at(current, 24)? as usize;
            let end = start
                .checked_add(u64_at(current, 32)? as usize)
                .ok_or("ELF overflow")?;
            return bytes
                .get(start..end)
                .ok_or_else(|| "truncated ELF text".into());
        }
    }
    Err("no .text section".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use iphem_core::{BusRegion, MachineConfig, Ram};

    #[test]
    fn assembly_and_conversion() {
        assert_eq!(
            encode_hex(&assemble("nop\nret\n").unwrap()),
            "1f2003d5c0035fd6"
        );
        assert!(assemble(".incbin \"/etc/passwd\"").is_err());
        assert!(assemble("bl unresolved_symbol").is_err());
        assert_eq!(
            convert(&json!({"data":"ffffffff"})).unwrap()["signed_le"],
            "-1"
        );
    }

    #[test]
    fn live_session_pauses_real_machine_and_requests_host_reset() {
        let mut machine = Machine::new(MachineConfig {
            trace: false,
            quantum: 1,
        });
        machine
            .bus
            .map(BusRegion {
                base: 0x1000,
                size: 0x1000,
                device: Box::new(Ram::new(0x1000, 0x1000)),
            })
            .unwrap();
        machine.mem_write(0x1000, 4, 0xd503_201f).unwrap();
        machine.cores.active_mut().state.pc = 0x1000;
        let mut server = DebugControlPlane::new(None, DebugServerConfig::live(0, "test"));
        assert!(server
            .command(&mut machine, &json!({"command":"attach"}))
            .is_ok());
        assert!(server
            .command(&mut machine, &json!({"command":"pause"}))
            .is_ok());
        assert!(server.step_target(&mut machine).is_none());
        server
            .command(
                &mut machine,
                &json!({"command":"breakpoint_add","address":"0x1000"}),
            )
            .unwrap();
        server
            .command(&mut machine, &json!({"command":"run","steps":10}))
            .unwrap();
        assert!(server.step_target(&mut machine).is_none());
        assert_eq!(server.status(&machine)["reason"], "breakpoint");
        server
            .command(&mut machine, &json!({"command":"step","count":1}))
            .unwrap();
        assert_eq!(machine.cores.active().state.pc, 0x1004);
        server
            .command(&mut machine, &json!({"command":"reset"}))
            .unwrap();
        assert_eq!(server.actions.pop_front(), Some(DebugAction::Reset));
    }
}
