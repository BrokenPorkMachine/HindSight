use std::any::Any;
use std::fmt;
pub type DeviceId = u32;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceResult {
    Value(u64),
    Ok,
    Fault,
    Event(DeviceEvent),
}
impl DeviceResult {
    /// Unwrap the `Value(u64)` payload; panics for `Ok`/`Fault`/`Event`.
    pub fn unwrap_u64(self) -> u64 {
        match self {
            DeviceResult::Value(v) => v,
            DeviceResult::Ok => panic!("DeviceResult::Ok carries no value"),
            DeviceResult::Fault => panic!("DeviceResult::Fault carries no value"),
            DeviceResult::Event(_) => panic!("DeviceResult::Event carries no value"),
        }
    }

    /// Unwrap the `Event(DeviceEvent)` payload; panics for other variants.
    pub fn unwrap_event(self) -> DeviceEvent {
        match self {
            DeviceResult::Event(e) => e,
            DeviceResult::Value(_) => panic!("DeviceResult::Value carries no event"),
            DeviceResult::Ok => panic!("DeviceResult::Ok carries no event"),
            DeviceResult::Fault => panic!("DeviceResult::Fault carries no event"),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceEvent {
    Interrupt(u32),
    InterruptTo(u32, Option<usize>),
    Fiq(u32),
    FiqTo(u32, Option<usize>),
    SError(u64),
    SErrorTo(u64, Option<usize>),
    DmaRequest { channel: u32 },
}
pub trait Device: Send + Any {
    fn id(&self) -> DeviceId;
    fn name(&self) -> &'static str;
    fn reset(&mut self);
    fn read(&mut self, offset: u64, size: usize) -> DeviceResult;
    fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult;
    fn tick(&mut self, cycles: u64) -> Option<DeviceEvent>;
    /// Assert a wired interrupt input on this device, such as a PPI entering
    /// an interrupt controller. Returns true when this device owns the line.
    fn signal_interrupt(&mut self, _irq: u32, _core: Option<usize>) -> bool {
        false
    }
    /// Set a level-sensitive FIQ source owned by this device/controller.
    /// `line` is controller-defined and `asserted` is recomputed as source
    /// state changes, allowing level interrupts to deassert correctly.
    fn set_fiq_line(&mut self, _line: u32, _core: Option<usize>, _asserted: bool) -> bool {
        false
    }
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
impl fmt::Display for DeviceEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
