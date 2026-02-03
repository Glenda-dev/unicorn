use super::log;
use glenda::interface::{DeviceService, PciService};

pub struct PciManager {
    ecam_base: usize,
}

impl PciManager {
    pub fn new(ecam_base: usize) -> Self {
        Self { ecam_base }
    }

    fn get_addr(&self, bus: u8, dev: u8, func: u8, offset: usize) -> usize {
        self.ecam_base
            + ((bus as usize) << 20)
            + ((dev as usize) << 15)
            + ((func as usize) << 12)
            + offset
    }
}

impl PciService for PciManager {
    fn read_config(&self, bus: u8, dev: u8, func: u8, offset: usize, size: usize) -> u32 {
        let addr = self.get_addr(bus, dev, func, offset);
        // In a real system, we'd need this mapped.
        // For now, we assume it's mapped or we are in a context where we can read it.
        match size {
            1 => unsafe { (addr as *const u8).read_volatile() as u32 },
            2 => unsafe { (addr as *const u16).read_volatile() as u32 },
            4 => unsafe { (addr as *const u32).read_volatile() },
            _ => 0,
        }
    }

    fn write_config(&mut self, bus: u8, dev: u8, func: u8, offset: usize, value: u32, size: usize) {
        let addr = self.get_addr(bus, dev, func, offset);
        match size {
            1 => unsafe { (addr as *mut u8).write_volatile(value as u8) },
            2 => unsafe { (addr as *mut u16).write_volatile(value as u16) },
            4 => unsafe { (addr as *mut u32).write_volatile(value) },
            _ => {}
        }
    }

    fn scan(&mut self, _dev_mgr: &mut dyn DeviceService) {
        log!("Scanning PCI bus...");
        for bus in 0..=0 {
            // Simplified
            for dev in 0..32 {
                for func in 0..8 {
                    let vendor_id = self.read_config(bus, dev, func, 0x00, 2) as u16;
                    if vendor_id == 0xFFFF {
                        continue;
                    }
                    let device_id = self.read_config(bus, dev, func, 0x02, 2) as u16;
                    log!(
                        "PCI: Found Device {}:{}.{} ID {:04x}:{:04x}",
                        bus,
                        dev,
                        func,
                        vendor_id,
                        device_id
                    );
                    // TODO: Create DeviceNode and add to dev_mgr
                }
            }
        }
    }
}
