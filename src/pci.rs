use crate::device::{Device, DeviceManager};
use alloc::vec::Vec;
use glenda::println;

// QEMU Virt PCIe ECAM Base
const ECAM_BASE: usize = 0x3000_0000;

pub struct PciManager {
    // We might need a reference to DeviceManager or just return a list
}

impl PciManager {
    pub fn new() -> Self {
        Self {}
    }

    pub fn scan(&mut self, _dev_mgr: &mut DeviceManager) {
        println!("Unicorn: Scanning PCI bus...");

        // Brute force scan (simplified)
        for bus in 0..=0 {
            // Just bus 0 for now
            for dev in 0..32 {
                for func in 0..8 {
                    let vendor_id = self.read_config_16(bus, dev, func, 0x00);
                    if vendor_id == 0xFFFF {
                        continue;
                    }

                    let device_id = self.read_config_16(bus, dev, func, 0x02);
                    println!(
                        "Unicorn: Found PCI Device {}:{}.{} ID {:04x}:{:04x}",
                        bus, dev, func, vendor_id, device_id
                    );

                    // TODO: Add to DeviceManager
                }
            }
        }
    }

    fn read_config_32(&self, bus: u8, dev: u8, func: u8, offset: usize) -> u32 {
        let addr = ECAM_BASE
            + ((bus as usize) << 20)
            + ((dev as usize) << 15)
            + ((func as usize) << 12)
            + offset;
        // unsafe { (addr as *const u32).read_volatile() }
        // We can't read if not mapped. Mocking for now.
        0xFFFF_FFFF
    }

    fn read_config_16(&self, bus: u8, dev: u8, func: u8, offset: usize) -> u16 {
        let val = self.read_config_32(bus, dev, func, offset & !3);
        let shift = (offset & 3) * 8;
        ((val >> shift) & 0xFFFF) as u16
    }
}
