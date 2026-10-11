use iphem_core::{
    BusRegion, CpuRunState, Device, DeviceEvent, DeviceResult, Machine, MachineConfig,
};
use std::any::Any;
use std::collections::{HashMap, VecDeque};

struct EventDevice {
    events: VecDeque<DeviceEvent>,
}

impl Device for EventDevice {
    fn id(&self) -> u32 {
        0x97
    }

    fn name(&self) -> &'static str {
        "phase97-events"
    }

    fn reset(&mut self) {}

    fn read(&mut self, _offset: u64, _size: usize) -> DeviceResult {
        DeviceResult::Ok
    }

    fn write(&mut self, _offset: u64, _size: usize, _value: u64) -> DeviceResult {
        DeviceResult::Ok
    }

    fn tick(&mut self, _cycles: u64) -> Option<DeviceEvent> {
        self.events.pop_front()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[test]
fn targeted_fiq_and_broadcast_serror_reach_cpu_pending_state() {
    let mut m = Machine::new(MachineConfig {
        trace: true,
        quantum: 1,
    });
    m.cores[2].run_state = CpuRunState::WaitingForInterrupt;
    m.bus
        .map(BusRegion {
            base: 0x9700,
            size: 0x100,
            device: Box::new(EventDevice {
                events: VecDeque::from([
                    DeviceEvent::FiqTo(17, Some(2)),
                    DeviceEvent::SError(0x155),
                ]),
            }),
        })
        .unwrap();

    m.tick_devices(1);
    assert_eq!(m.cores[2].pending_fiq, Some(17));
    assert_eq!(m.cores[2].run_state, CpuRunState::Running);
    assert_eq!(m.cores.active().pending_fiq, None);

    m.tick_devices(1);
    assert_eq!(m.cores.active().pending_serror, Some(0x155));
    assert!(m.cores.iter().all(|cpu| cpu.pending_serror == Some(0x155)));
}

#[test]
fn out_of_range_targeted_interrupts_do_not_broadcast_to_active_cpu() {
    let mut m = Machine::new(MachineConfig {
        trace: true,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base: 0x9800,
            size: 0x100,
            device: Box::new(EventDevice {
                events: VecDeque::from([
                    DeviceEvent::InterruptTo(70, Some(usize::MAX)),
                    DeviceEvent::FiqTo(71, Some(usize::MAX)),
                    DeviceEvent::SErrorTo(0x155, Some(usize::MAX)),
                ]),
            }),
        })
        .unwrap();

    m.tick_devices(1);
    m.tick_devices(1);
    m.tick_devices(1);

    assert_eq!(m.cores.active().pending_irq, None);
    assert_eq!(m.cores.active().pending_fiq, None);
    assert_eq!(m.cores.active().pending_serror, None);
    assert!(m.cores.iter().all(|cpu| cpu.pending_irq.is_none()));
    assert!(m.cores.iter().all(|cpu| cpu.pending_fiq.is_none()));
    assert!(m.cores.iter().all(|cpu| cpu.pending_serror.is_none()));
}

#[test]
fn snapshot_retains_all_pending_async_exceptions() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().pending_irq = Some(11);
    m.cores.active_mut().pending_fiq = Some(12);
    m.cores.active_mut().pending_serror = Some(0x155);

    let snapshot = iphem_core::MachineSnapshot::new(&mut m, HashMap::new()).unwrap();
    assert_eq!(snapshot.active_cpu().pending_irq, Some(11));
    assert_eq!(snapshot.active_cpu().pending_fiq, Some(12));
    assert_eq!(snapshot.active_cpu().pending_serror, Some(0x155));
}
