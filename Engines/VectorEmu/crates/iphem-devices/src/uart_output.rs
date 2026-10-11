//! Shared UART draining and host-only source labels. Raw guest bytes stay intact.

use crate::{BasebandUart, S5lUart, SepUart, SerialConsole, Uart, UartWindow};
use iphem_core::device::Device;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UartSource {
    Main,
    SerialConsole,
    Window,
    Kernel,
    Sep,
    Baseband,
}

impl UartSource {
    fn tag(self, address: u64) -> &'static str {
        match self {
            Self::Main | Self::SerialConsole | Self::Window => "[UART] ",
            Self::Kernel => match address {
                0x235100000 | 0x235200000 => "[UART0] ",
                0x235204000 => "[UART1] ",
                0x235208000 => "[UART2] ",
                0x23520c000 => "[UART3] ",
                _ => "[KRNL] ",
            },
            Self::Sep => "[SEP] ",
            Self::Baseband => "[BB] ",
        }
    }
}

/// Drain a supported UART exactly once; other devices are left untouched.
pub fn drain_uart(device: &mut dyn Device) -> Option<(UartSource, Vec<u8>)> {
    let output = if let Some(uart) = device.as_any_mut().downcast_mut::<Uart>() {
        (UartSource::Main, uart.drain_output())
    } else if let Some(uart) = device.as_any_mut().downcast_mut::<SerialConsole>() {
        (UartSource::SerialConsole, uart.take_log())
    } else if let Some(uart) = device.as_any_mut().downcast_mut::<UartWindow>() {
        (UartSource::Window, uart.drain())
    } else if let Some(uart) = device.as_any_mut().downcast_mut::<SepUart>() {
        (UartSource::Sep, uart.drain_output())
    } else if let Some(uart) = device.as_any_mut().downcast_mut::<S5lUart>() {
        (UartSource::Kernel, uart.drain_output())
    } else {
        let uart = device.as_any_mut().downcast_mut::<BasebandUart>()?;
        (UartSource::Baseband, uart.drain_output())
    };
    if output.1.is_empty() {
        None
    } else {
        Some(output)
    }
}

/// Streaming host formatter. Source switches end an unfinished line immediately,
/// so one device cannot append text under another device's label. Ordering is
/// drain order, not a claim of cycle-accurate ordering between device FIFOs.
#[derive(Default)]
pub struct TaggedUartOutput {
    current: Option<(UartSource, u64)>,
    line_open: bool,
    carriage_return: Option<(UartSource, u64)>,
}

impl TaggedUartOutput {
    /// `address` distinguishes separate UARTs of the same type on the bus.
    pub fn append(&mut self, source: UartSource, address: u64, bytes: &[u8]) -> String {
        let mut output = String::new();
        let identity = (source, address);
        for &byte in bytes {
            if byte == b'\n' && self.carriage_return == Some(identity) {
                self.carriage_return = None;
                continue;
            }
            self.carriage_return = None;
            if self.current != Some(identity) && self.line_open {
                output.push('\n');
                self.line_open = false;
            }
            self.current = Some(identity);
            if !self.line_open {
                output.push_str(source.tag(address));
                self.line_open = true;
            }
            match byte {
                b'\r' | b'\n' => {
                    output.push('\n');
                    self.line_open = false;
                    if byte == b'\r' {
                        self.carriage_return = Some(identity);
                    }
                }
                b'\t' | 0x20..=0x7e => output.push(byte as char),
                _ => {
                    use std::fmt::Write;
                    let _ = write!(output, "\\x{byte:02x}");
                }
            }
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uart::{S5L_UCON, S5L_UTXH};

    #[test]
    fn partial_lines_source_switches_and_split_crlf() {
        let mut formatter = TaggedUartOutput::default();
        let mut text = formatter.append(UartSource::Main, 1, b"bo");
        text += &formatter.append(UartSource::Main, 1, b"ot\r");
        text += &formatter.append(UartSource::Main, 1, b"\nnext");
        text += &formatter.append(UartSource::Sep, 2, b"secure");
        text += &formatter.append(UartSource::Baseband, 3, b"radio\n");
        text += &formatter.append(UartSource::Main, 1, b" ready\n\xff");
        assert_eq!(
            text,
            "[UART] boot\n[UART] next\n[SEP] secure\n[BB] radio\n[UART]  ready\n[UART] \\xff"
        );
    }

    #[test]
    fn empty_drains_and_distinct_uart_addresses() {
        let mut formatter = TaggedUartOutput::default();
        assert_eq!(formatter.append(UartSource::Main, 1, b"A"), "[UART] A");
        assert_eq!(formatter.append(UartSource::Sep, 2, b""), "");
        assert_eq!(formatter.append(UartSource::Main, 1, b"B"), "B");
        assert_eq!(formatter.append(UartSource::Main, 2, b"C"), "\n[UART] C");
    }

    #[test]
    fn auxiliary_drains_preserve_raw_bytes_and_clear_fifos() {
        let mut sep = SepUart::new(2, "SEP");
        let mut bb = BasebandUart::new(3, "BB");
        sep.set_console_output(false);
        bb.set_console_output(false);
        sep.write(0, 1, 0xff);
        bb.write(0, 1, b'B' as u64);
        assert_eq!(drain_uart(&mut sep), Some((UartSource::Sep, vec![0xff])));
        assert_eq!(
            drain_uart(&mut bb),
            Some((UartSource::Baseband, b"B".to_vec()))
        );
        assert_eq!(drain_uart(&mut sep), None);
        assert_eq!(drain_uart(&mut bb), None);
    }

    #[test]
    fn kernel_s5l_uart_drains_as_kernel_source() {
        let mut uart = S5lUart::new(9, "uart1");
        uart.write(S5L_UCON, 4, 0x1245);
        // Configuration never surfaces; the TX holding write does.
        assert_eq!(drain_uart(&mut uart), None);
        uart.write(S5L_UTXH, 1, b'X' as u64);
        assert_eq!(
            drain_uart(&mut uart),
            Some((UartSource::Kernel, b"X".to_vec()))
        );
        assert_eq!(drain_uart(&mut uart), None);
        let mut formatter = TaggedUartOutput::default();
        assert_eq!(
            formatter.append(UartSource::Kernel, 0x235204000, b"darwin\n"),
            "[UART1] darwin\n"
        );
        assert_eq!(
            formatter.append(UartSource::Kernel, 0x235200000, b"kernel\n"),
            "[UART0] kernel\n"
        );
    }
}
