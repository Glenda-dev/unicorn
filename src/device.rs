extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Clone)]
pub enum DeviceType {
    Pci {
        vendor_id: u16,
        device_id: u16,
        bus: u8,
        dev: u8,
        func: u8,
    },
    Platform {
        name: String,
        compatible: String,
        mmio: Vec<(usize, usize)>, // (paddr, size)
        irqs: Vec<usize>,
    },
}

#[derive(Debug, Clone)]
pub struct Device {
    pub id: usize,
    pub dev_type: DeviceType,
}

pub struct DeviceManager {
    devices: Vec<Device>,
    next_id: usize,
}

impl DeviceManager {
    pub fn new() -> Self {
        Self { devices: Vec::new(), next_id: 1 }
    }

    pub fn add_device(&mut self, dev_type: DeviceType) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.devices.push(Device { id, dev_type });
        id
    }

    pub fn find_by_name(&self, name: &str) -> Option<usize> {
        for dev in &self.devices {
            match &dev.dev_type {
                DeviceType::Platform { name: n, .. } if n == name => return Some(dev.id),
                _ => {}
            }
        }
        None
    }

    pub fn get_device(&self, id: usize) -> Option<&Device> {
        self.devices.iter().find(|d| d.id == id)
    }
}
