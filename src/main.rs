#![no_std]
#![no_main]
#![allow(dead_code)]

extern crate alloc;

use glenda::cap::CapPtr;
use glenda::cap::pagetable::perms;
use glenda::console;
use glenda::initrd::Initrd;
use glenda::ipc::{MsgTag, UTCB};
use glenda::println;
use glenda::protocol::factotum;
use glenda::protocol::unicorn as protocol;

mod device;
mod dma;
mod dtb;
mod manifest;
mod pci;

use device::DeviceManager;
use dma::DmaManager;
use manifest::Manifest;
use pci::PciManager;

const UNICORN_ENDPOINT_SLOT: usize = 11; // Self endpoint
const MANIFEST_ADDR: usize = 0x2000_0000;
const INITRD_VA: usize = 0x4000_0000;

#[unsafe(no_mangle)]
fn main() -> ! {
    // Initialize logging (assuming cap 5 is console)
    console::init(CapPtr(5));
    println!("Unicorn: Starting Device Driver Manager...");

    let factotum = CapPtr(10);

    // Request our own endpoint from Factotum (Slot 10)
    let msg_tag = MsgTag::new(factotum::REQUEST_CAP, 4);
    // Type 2 (Endpoint), id=0, dest=UNICORN_ENDPOINT_SLOT, target=0 (self)
    let args = [2, 0, UNICORN_ENDPOINT_SLOT, 0, 0, 0];
    factotum.ipc_call(msg_tag, &args);
    let ret = UTCB::current().mrs_regs[0];
    if ret != 0 {
        println!("Unicorn: Failed to allocate endpoint");
        loop {}
    }

    // Request Initrd Cap (Slot 20)
    let initrd_cap = CapPtr(20);
    let msg_tag = MsgTag::new(factotum::REQUEST_CAP, 4);
    // Type 3 (Initrd), id=0, dest=20, target=0
    let args = [3, 0, 20, 0, 0, 0];
    factotum.ipc_call(msg_tag, &args);
    if UTCB::current().mrs_regs[0] != 0 {
        println!("Unicorn: Failed to get Initrd Cap");
        loop {}
    }

    // Map Initrd
    let vspace = CapPtr(1);
    vspace.pagetable_map(initrd_cap, INITRD_VA, perms::READ);

    let total_size_ptr = (INITRD_VA + 8) as *const u32;
    let total_size = unsafe { *total_size_ptr } as usize;
    println!("Unicorn: Initrd size: {}", total_size);

    let initrd_slice = unsafe { core::slice::from_raw_parts(INITRD_VA as *const u8, total_size) };
    let initrd = Initrd::new(initrd_slice).expect("Unicorn: Failed to parse initrd");

    // Parse Manifest
    let manifest_slice = unsafe { core::slice::from_raw_parts(MANIFEST_ADDR as *const u8, 4096) };
    let manifest = Manifest::parse(manifest_slice);
    println!("Unicorn: Parsed manifest with {} drivers", manifest.drivers.len());

    let mut pci_mgr = PciManager::new();
    let mut _dma_mgr = DmaManager::new();
    let mut dev_mgr = DeviceManager::new();

    // Scan PCI bus
    pci_mgr.scan(&mut dev_mgr);

    // Spawn Drivers from Manifest
    for driver in manifest.drivers {
        println!("Unicorn: Processing driver for '{}': {}", driver.compatible, driver.binary);

        // Find binary in Initrd
        if let Some(entry) = initrd.entries.iter().find(|e| e.name == driver.binary) {
            spawn_driver(factotum, initrd_cap, entry);
        } else {
            println!("Unicorn: Binary {} not found in initrd", driver.binary);
        }
    }

    let endpoint = CapPtr(UNICORN_ENDPOINT_SLOT);
    println!("Unicorn: Listening on endpoint {}", UNICORN_ENDPOINT_SLOT);

    loop {
        let _badge = endpoint.ipc_recv();
        let utcb = UTCB::current();
        let tag = utcb.msg_tag;
        let label = tag.label();

        if label != protocol::UNICORN_PROTO {
            println!("Unicorn: Unknown protocol label: {:#x}", label);
            continue;
        }

        let method = utcb.mrs_regs[0];
        let ret = match method {
            protocol::SCAN_BUS => {
                pci_mgr.scan(&mut dev_mgr);
                0
            }
            protocol::LIST_DEVICES => {
                // TODO: Return list of devices
                0
            }
            protocol::MAP_MMIO => {
                // TODO: Map MMIO for a device
                0
            }
            protocol::GET_IRQ => {
                let irq = utcb.mrs_regs[1];
                let dest_slot = utcb.mrs_regs[2];
                let driver_pid = _badge;

                println!("Unicorn: GET_IRQ {} for PID {} at slot {}", irq, driver_pid, dest_slot);

                let factotum = CapPtr(10);
                let msg_tag = MsgTag::new(factotum::REQUEST_CAP, 4);
                // Type 1 (IRQ), id=irq, dest=dest_slot, target=driver_pid
                let args = [1, irq, dest_slot, driver_pid, 0, 0];
                factotum.ipc_call(msg_tag, &args);
                let ret = UTCB::current().mrs_regs[0];

                ret
            }
            protocol::ALLOC_DMA => {
                // TODO: Allocate DMA memory
                0
            }
            _ => {
                println!("Unicorn: Unknown method: {}", method);
                -1isize as usize
            }
        };

        utcb.mrs_regs[0] = ret;
        let args = [ret, 0, 0, 0, 0];
        endpoint.ipc_reply(MsgTag::new(label, 1), &args);
    }
}

fn spawn_driver(factotum: CapPtr, initrd_cap: CapPtr, entry: &glenda::initrd::Entry) {
    println!("Unicorn: Spawning {}", entry.name);

    // SPAWN
    let ipc_buf = glenda::ipc::utcb::get_ipc_buffer();
    ipc_buf.clear();
    ipc_buf.append_str(&entry.name);

    let msg_tag = MsgTag::new(factotum::SPAWN, 2);
    let args = [entry.name.len(), 0, 0, 0, 0, 0];
    factotum.ipc_call(msg_tag, &args);
    let pid = UTCB::current().mrs_regs[0];

    if pid == usize::MAX {
        println!("Unicorn: Failed to spawn {}", entry.name);
        return;
    }
    println!("Unicorn: Spawned {} with PID {}", entry.name, pid);

    // PROCESS_LOAD_IMAGE
    // args: [pid, frame_cap, offset, len, load_addr]
    // frame_cap is initrd_cap (20)
    let msg_tag = MsgTag::new(factotum::PROCESS_LOAD_IMAGE, 5);
    let args = [pid, initrd_cap.0, entry.offset, entry.size, 0x10000, 0];
    factotum.ipc_call(msg_tag, &args);
    let ret = UTCB::current().mrs_regs[0];

    if ret != 0 {
        println!("Unicorn: Failed to load image for {}", entry.name);
        return;
    }

    // PROCESS_START
    let msg_tag = MsgTag::new(factotum::PROCESS_START, 3);
    let args = [pid, 0x10000, 0x8000_0000, 0, 0, 0];
    factotum.ipc_call(msg_tag, &args);
    println!("Unicorn: Started {}", entry.name);
}
