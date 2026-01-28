use crate::device::{DeviceManager, DeviceType};
use alloc::string::String;
use alloc::vec::Vec;
use fdt::Fdt;
use glenda::println;

pub struct DtbManager<'a> {
    fdt: Fdt<'a>,
}

impl<'a> DtbManager<'a> {
    pub fn new(dtb_slice: &'a [u8]) -> Option<Self> {
        let fdt = Fdt::new(dtb_slice).ok()?;
        Some(Self { fdt })
    }

    pub fn scan(&self, dev_mgr: &mut DeviceManager) {
        println!("Unicorn: Scanning Device Tree...");
        println!("Model: {}", self.fdt.root().model());

        for node in self.fdt.all_nodes() {
            if let Some(compatible) = node.compatible() {
                let name = node.name;
                let first_compat = compatible.first();
                println!("Unicorn: Found DTB Node: {} (compatible: {})", name, first_compat);

                let mut mmio = Vec::new();
                if let Some(reg) = node.reg() {
                    for range in reg {
                        mmio.push((range.starting_address as usize, range.size.unwrap_or(0)));
                    }
                }

                let mut irqs = Vec::new();
                if let Some(interrupts) = node.interrupts() {
                    for irq in interrupts {
                        irqs.push(irq);
                    }
                }

                dev_mgr.add_device(DeviceType::Platform {
                    name: String::from(name),
                    compatible: String::from(first_compat),
                    mmio,
                    irqs,
                });
            }
        }
    }
}
