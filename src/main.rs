#![no_std]
#![no_main]
#![allow(dead_code)]

extern crate alloc;

use glenda::cap::CapPtr;
use glenda::ipc::{MsgTag, UTCB};
use glenda::log;
use glenda::println;
use glenda::protocol::factotum;
use glenda::protocol::unicorn as protocol;

mod device;
mod dma;
mod dtb;
mod pci;

use device::DeviceManager;
use dma::DmaManager;
use dtb::DtbManager;
use pci::PciManager;

const UNICORN_ENDPOINT_SLOT: usize = 11; // Self endpoint

#[unsafe(no_mangle)]
fn main() -> ! {
    // Initialize logging (assuming cap 5 is console)
    log::init(CapPtr(5));
    println!("Unicorn: Starting Device Driver Manager...");

    // Request our own endpoint from Factotum (Slot 10)
    let factotum = CapPtr(10);
    let msg_tag = MsgTag::new(factotum::REQUEST_CAP, 4);
    // Type 2 (Endpoint), id=0, dest=UNICORN_ENDPOINT_SLOT, target=0 (self)
    let args = [2, 0, UNICORN_ENDPOINT_SLOT, 0, 0, 0];
    factotum.ipc_call(msg_tag, &args);
    let ret = UTCB::current().mrs_regs[0];
    if ret != 0 {
        println!("Unicorn: Failed to allocate endpoint");
        loop {}
    }

    let mut pci_mgr = PciManager::new();
    let mut _dma_mgr = DmaManager::new();
    let mut dev_mgr = DeviceManager::new();

    // Scan PCI bus
    pci_mgr.scan(&mut dev_mgr);

    // TODO: Map DTB and scan
    // let dtb_va = ...;
    // let dtb_size = ...;
    // let dtb_slice = unsafe { core::slice::from_raw_parts(dtb_va as *const u8, dtb_size) };
    // if let Some(dtb_mgr) = DtbManager::new(dtb_slice) {
    //     dtb_mgr.scan(&mut dev_mgr);
    // }

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
