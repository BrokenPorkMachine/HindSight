use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DfuState {
    Idle,
    Downloading,
    ManifestReceived,
    ReadyForBoot,
    Error,
}

pub struct DfuHandler {
    state: Arc<Mutex<DfuState>>,
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl DfuHandler {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(DfuState::Idle)),
            buffer: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn handle_request(&self, request: u8, data: &[u8]) -> Vec<u8> {
        let mut state = self.state.lock().unwrap();
        match request {
            0x01 => { // DFU_DETACH
                *state = DfuState::Idle;
                vec![0]
            }
            0x02 => { // DFU_DOWNLOAD
                let mut buf = self.buffer.lock().unwrap();
                buf.extend_from_slice(data);
                *state = DfuState::Downloading;
                vec![0]
            }
            0x03 => { // DFU_GETSTATUS
                let status = match *state {
                    DfuState::Idle => 0x00,
                    DfuState::Downloading => 0x01,
                    DfuState::ManifestReceived => 0x02,
                    DfuState::ReadyForBoot => 0x03,
                    DfuState::Error => 0xFF,
                };
                vec![status]
            }
            _ => vec![0x01], // STALL
        }
    }

    pub fn get_buffer(&self) -> Vec<u8> {
        self.buffer.lock().unwrap().clone()
    }
}
