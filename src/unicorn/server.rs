use crate::UnicornManager;
use crate::layout::{
    BOOTINFO_ADDR, BOOTINFO_SLOT, IRQ_SLOT, MANIFEST_SLOT, MMIO_SLOT, RESOURCE_ADDR,
};
use crate::log;
use glenda::arch::mem::PGSIZE;
use glenda::cap::{CapPtr, Endpoint, Frame, Reply};
use glenda::error::Error;
use glenda::interface::{DeviceService, MemoryService, ResourceService, SystemService};
use glenda::ipc::server::{handle_call, handle_cap_call};
use glenda::ipc::{Badge, MsgTag, UTCB};
use glenda::protocol::device;
use glenda::protocol::resource::{DEVICE_ENDPOINT, ResourceType};
use glenda::protocol::{self, DEVICE_PROTO};

impl<'a> SystemService for UnicornManager<'a> {
    fn init(&mut self) -> Result<(), Error> {
        log!("Loading config ...");
        let (frame, size) =
            self.res_client.get_config(Badge::null(), "drivers.json", MANIFEST_SLOT)?;
        self.res_client.mmap(Badge::null(), frame, RESOURCE_ADDR, size)?;
        let data = unsafe { core::slice::from_raw_parts(RESOURCE_ADDR as *const u8, size) };
        self.config = serde_json::from_slice(data).map_err(|_| Error::InvalidConfig)?;
        self.res_client.munmap(Badge::null(), RESOURCE_ADDR, size)?;

        log!("Loading Bootinfo ...");
        let frame =
            self.res_client.get_cap(Badge::null(), ResourceType::Bootinfo, 0, BOOTINFO_SLOT)?;
        self.res_client.mmap(Badge::null(), Frame::from(frame), BOOTINFO_ADDR, PGSIZE)?;

        self.res_client.get_cap(Badge::null(), ResourceType::Mmio, 0, MMIO_SLOT)?;
        self.res_client.get_cap(Badge::null(), ResourceType::Irq, 0, IRQ_SLOT)?;

        self.init_root_platform()?;

        // Get MMIO and IRQ capabilities (CNode)
        self.scan_platform(Badge::null())?;

        self.print_tree();
        Ok(())
    }

    fn listen(&mut self, ep: Endpoint, reply: CapPtr, recv: CapPtr) -> Result<(), Error> {
        self.endpoint = ep;
        self.reply = Reply::from(reply);
        self.recv = recv;
        self.res_client.register_cap(
            Badge::null(),
            ResourceType::Endpoint,
            DEVICE_ENDPOINT,
            ep.cap(),
        )?;
        Ok(())
    }

    fn run(&mut self) -> Result<(), Error> {
        self.running = true;
        while self.running {
            let mut utcb = unsafe { UTCB::new() };
            utcb.clear();
            utcb.set_reply_window(self.reply.cap());
            utcb.set_recv_window(self.recv);
            match self.endpoint.recv(&mut utcb) {
                Ok(_) => {}
                Err(e) => {
                    log!("Recv error: {:?}", e);
                    continue;
                }
            };

            let res = self.dispatch(&mut utcb);
            if let Err(e) = res {
                if e == Error::Success {
                    continue;
                }
                log!("Failed to dispatch message: {:?}", e);
                utcb.set_msg_tag(MsgTag::err());
                utcb.set_mr(0, e as usize);
            }

            self.reply(&mut utcb)?;
        }
        Ok(())
    }

    fn dispatch(&mut self, utcb: &mut UTCB) -> Result<(), Error> {
        let badge = utcb.get_badge();
        let tag = utcb.get_msg_tag();
        let label = tag.label();
        let proto = tag.proto();
        let flags = tag.flags();
        let mrs = utcb.get_mrs();
        let size = utcb.get_size();
        log!(
            "Received message: badge={}, label={:#x}, proto={:#x}, flags={}, utcb.mrs_regs={:?}, size={}",
            badge,
            label,
            proto,
            flags,
            mrs,
            size
        );

        glenda::ipc_dispatch! {
            self, utcb,
            (protocol::KERNEL_PROTO, protocol::kernel::NOTIFY) => |s: &mut Self, u: &mut UTCB| {
                let irq = u.get_badge().bits();
                s.handle_irq(irq)
            },
            (DEVICE_PROTO, device::REPORT) => |s: &mut Self, u: &mut UTCB| {
                handle_call(u, |u| {
                    let desc = unsafe { u.read_postcard()? };
                    s.report(badge, desc)
                })
            },
            (DEVICE_PROTO, device::GET_MMIO) => |s: &mut Self, u: &mut UTCB| {
                handle_cap_call(u, |u| {
                    let id = u.get_mr(0);
                    let (frame, paddr, size) = s.get_mmio(badge, id)?;
                    u.set_mr(0, paddr);
                    u.set_mr(1, size);
                    Ok(frame.cap())
                })
            },
            (DEVICE_PROTO, device::GET_IRQ) => |s: &mut Self, u: &mut UTCB| {
                handle_cap_call(u, |u| {
                    let id = u.get_mr(0);
                    let handler = s.get_irq(badge, id)?;
                    Ok(handler.cap())
                })
            },
            (DEVICE_PROTO, device::SCAN_PLATFORM) => |s: &mut Self, u: &mut UTCB| {
                handle_call(u, |_| s.scan_platform(badge))
            },
        }
    }

    fn reply(&mut self, utcb: &mut UTCB) -> Result<(), Error> {
        self.reply.reply(utcb)
    }

    fn stop(&mut self) {
        self.running = false;
    }
}

impl<'a> UnicornManager<'a> {
    pub fn handle_irq(&mut self, irq: usize) -> Result<(), Error> {
        if let Some(&slot) = self.irq_caps.get(&irq) {
            let handler = glenda::cap::IrqHandler::from(slot);
            log!("IRQ {} received", irq);
            handler.ack()?;
        } else {
            log!("Unknown IRQ notification: {}", irq);
        }
        Ok(())
    }
}
