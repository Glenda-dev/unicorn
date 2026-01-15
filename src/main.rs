#![no_std]
#![no_main]
#![allow(dead_code)]

extern crate alloc;

mod device;
mod dma;
mod dtb;
mod layout;
mod pci;

use crate::device::DeviceManager;
use crate::dma::DmaManager;
use crate::layout::{
    FACTOTUM_CAP, INITRD_VA, MANIFEST_ADDR, UNICORN_ENDPOINT_CAP, UNICORN_ENDPOINT_SLOT,
};
use crate::pci::PciManager;
use glenda::cap::{CapPtr, Endpoint};
use glenda::initrd::Initrd;
use glenda::ipc::utcb;
use glenda::ipc::{MsgTag, UTCB};
use glenda::manifest::Manifest;
use glenda::mem::ENTRY_VA;
use glenda::protocol::factotum;
use glenda::protocol::unicorn as protocol;

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => ({
        glenda::println!("Unicorn: {}", format_args!($($arg)*));
    })
}

#[unsafe(no_mangle)]
fn main() -> ! {
    log!("Starting Unicorn Device Driver Manager...");

    let factotum = FACTOTUM_CAP;

    unimplemented!();

    let total_size_ptr = (INITRD_VA + 8) as *const u32;
    let total_size = unsafe { *total_size_ptr } as usize;
    log!("Initrd size: {}", total_size);

    let initrd_slice = unsafe { core::slice::from_raw_parts(INITRD_VA as *const u8, total_size) };
    let initrd = Initrd::new(initrd_slice).expect("Unicorn: Failed to parse initrd");

    // TODO: Parse Manifest
    let manifest_slice = unsafe { core::slice::from_raw_parts(MANIFEST_ADDR as *const u8, 4096) };
    let manifest = Manifest::parse(manifest_slice);
    log!("Parsed manifest with {} drivers", manifest.driver.len());

    let mut pci_mgr = PciManager::new();
    let mut _dma_mgr = DmaManager::new();
    let mut dev_mgr = DeviceManager::new();

    // Scan PCI bus
    pci_mgr.scan(&mut dev_mgr);

    // Spawn Drivers from Manifest
    for driver in manifest.driver {
        log!("Processing driver for '{:?}': {}", driver.compatible, driver.binary);

        // Find binary in Initrd
        if let Some(entry) = initrd.entries.iter().find(|e| e.name == driver.binary) {
            spawn_driver(factotum, entry);
        } else {
            log!("Binary {} not found in initrd", driver.binary);
        }
    }

    let endpoint = UNICORN_ENDPOINT_CAP;
    log!("Listening on endpoint {}", UNICORN_ENDPOINT_SLOT);

    loop {
        let _badge = endpoint.recv(0);
        let utcb = UTCB::current();
        let tag = utcb.msg_tag;
        let label = tag.label();

        if label != protocol::UNICORN_PROTO {
            log!("Unknown protocol label: {:#x}", label);
            continue;
        }

        let method = utcb.mrs_regs[0];
        let ret =
            match method {
                protocol::SCAN_BUS => {
                    pci_mgr.scan(&mut dev_mgr);
                    0
                }
                protocol::LIST_DEVICES => {
                    // TODO: Return list of devices
                    0
                }
                protocol::GET_DEVICE_BY_NAME => {
                    let name_len = utcb.mrs_regs[1];
                    if let Some(name) = utcb.read_str(0, name_len) {
                        if let Some(id) = dev_mgr.find_by_name(&name) { id } else { usize::MAX }
                    } else {
                        usize::MAX
                    }
                }
                protocol::MAP_MMIO => {
                    let device_id = utcb.mrs_regs[1];
                    let mmio_index = utcb.mrs_regs[2];
                    let dest_slot = utcb.mrs_regs[3];

                    if let Some(dev) = dev_mgr.get_device(device_id) {
                        match &dev.dev_type {
                            device::DeviceType::Platform { mmio, .. } => {
                                if mmio_index < mmio.len() { unimplemented!() } else { usize::MAX }
                            }
                            _ => usize::MAX,
                        }
                    } else {
                        usize::MAX
                    }
                }
                protocol::GET_IRQ => {
                    let device_id = utcb.mrs_regs[1];
                    let irq_index = utcb.mrs_regs[2];
                    let dest_slot = utcb.mrs_regs[3];
                    let driver_pid = _badge;

                    if let Some(dev) = dev_mgr.get_device(device_id) {
                        match &dev.dev_type {
                            device::DeviceType::Platform { irqs, .. } => {
                                if irq_index < irqs.len() {
                                    let irq = irqs[irq_index];
                                    log!(
                                        "GET_IRQ {} for PID {} at slot {}",
                                        irq,
                                        driver_pid,
                                        dest_slot
                                    );
                                    //TODO: Allocate IRQ capability
                                    unimplemented!()
                                } else {
                                    usize::MAX
                                }
                            }
                            _ => usize::MAX,
                        }
                    } else {
                        usize::MAX
                    }
                }
                protocol::ALLOC_DMA => {
                    // TODO: Allocate DMA memory
                    0
                }
                _ => {
                    log!("Unknown method: {}", method);
                    -1isize as usize
                }
            };

        utcb.mrs_regs[0] = ret;
        let args = [ret, 0, 0, 0, 0, 0, 0];
        unimplemented!()
    }
}

fn spawn_driver(factotum: Endpoint, entry: &glenda::initrd::Entry) {
    log!("Spawning {}", entry.name);

    // SPAWN
    let utcb = utcb::get();
    utcb.clear();
    utcb.append_str(&entry.name);

    let msg_tag = MsgTag::new(factotum::SPAWN, 2);
    let args = [entry.name.len(), 0, 0, 0, 0, 0, 0];
    factotum.call(msg_tag, args);
    let pid = UTCB::current().mrs_regs[0];

    if pid == usize::MAX {
        log!("Failed to spawn {}", entry.name);
        return;
    }
    log!("Spawned {} with PID {}", entry.name, pid);

    // PROCESS_LOAD_IMAGE
    // args: [pid, frame_cap, offset, len, load_addr]
    // frame_cap is initrd_cap (20)
    let msg_tag = MsgTag::new(factotum::PROCESS_LOAD_IMAGE, 5);
    let args = [pid, 0, entry.offset, entry.size, ENTRY_VA, 0, 0];
    factotum.call(msg_tag, args);
    let ret = UTCB::current().mrs_regs[0];

    if ret != 0 {
        log!("Failed to load image for {}", entry.name);
        return;
    }

    // SHARE UNICORN ENDPOINT
    let utcb = utcb::get();
    utcb.clear();
    utcb.cap_transfer = UNICORN_ENDPOINT_CAP.cap();
    let mut tag = MsgTag::new(factotum::FACTOTUM_PROTO, 3);
    tag.set_has_cap();
    let args = [factotum::SHARE_CAP, 11, pid, 0, 0, 0, 0];
    factotum.call(tag, args);
    // PROCESS_START
    let msg_tag = MsgTag::new(factotum::PROCESS_START, 3);
    let args = [pid, ENTRY_VA, 0x8000_0000, 0, 0, 0, 0];
    factotum.call(msg_tag, args);
    log!("Started {}", entry.name);
}
