extern crate alloc;
use alloc::vec::Vec;

#[derive(Debug, Clone)]
pub struct Device {
    pub vendor_id: u16,
    pub device_id: u16,
    pub bus: u8,
    pub dev: u8,
    pub func: u8,
}

pub struct DeviceManager {
    devices: Vec<Device>,
}

impl DeviceManager {
    pub fn new() -> Self {
        Self { devices: Vec::new() }
    }

    pub fn add_device(&mut self, device: Device) {
        self.devices.push(device);
    }
}
