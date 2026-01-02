use crate::device::{Device, DeviceManager};
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

    pub fn scan(&self, _dev_mgr: &mut DeviceManager) {
        println!("Unicorn: Scanning Device Tree...");
        println!("Model: {}", self.fdt.root().model());

        for node in self.fdt.all_nodes() {
            if let Some(compatible) = node.compatible() {
                let name = node.name;
                let first_compat = compatible.first();
                println!("Unicorn: Found DTB Node: {} (compatible: {})", name, first_compat);

                if let Some(reg) = node.reg() {
                    for range in reg {
                        println!(
                            "  Reg: {:#x} - {:#x}",
                            range.starting_address as usize,
                            range.starting_address as usize + range.size.unwrap_or(0)
                        );
                    }
                }

                if let Some(interrupts) = node.interrupts() {
                    for irq in interrupts {
                        println!("  IRQ: {}", irq);
                    }
                }
            }
        }
    }
}
