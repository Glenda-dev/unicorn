use crate::UnicornManager;
use crate::layout::{BOOTINFO_ADDR, BOOTINFO_SLOT, MANIFEST_SLOT, RESOURCE_ADDR};
use glenda::arch::mem::PGSIZE;
use glenda::cap::{CapPtr, Endpoint, Frame, Reply};
use glenda::error::Error;
use glenda::interface::{
    DeviceService, InitService, MemoryService, ResourceService, SystemService,
};
use glenda::ipc::server::{handle_buffer_call, handle_call, handle_cap_call, handle_notify};
use glenda::ipc::{Badge, MsgTag, UTCB};
use glenda::protocol::device;
use glenda::protocol::init::ServiceState;
use glenda::protocol::resource::{DEVICE_ENDPOINT, ResourceType};
use glenda::protocol::{self, DEVICE_PROTO};
use glenda::utils::manager::CSpaceService;

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

        self.init_root_platform()?;
        self.init_initrd_device()?;

        // Get MMIO and IRQ capabilities (CNode)
        self.scan_platform(Badge::null())?;
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
        self.init_client.report_service(Badge::null(), ServiceState::Running)?;
        self.running = true;
        while self.running {
            while let Some(id) = self.spawn_queue.pop_front() {
                if let Err(e) = self.start_driver(id) {
                    error!("Failed to start driver for device {}: {:?}", id.index, e);
                }
            }

            let mut utcb = unsafe { UTCB::new() };
            utcb.clear();
            let _ = self.cspace_mgr.root().delete(self.recv);
            utcb.set_reply_window(self.reply.cap());
            utcb.set_recv_window(self.recv);
            match self.endpoint.recv(&mut utcb) {
                Ok(_) => {}
                Err(e) => {
                    error!("Recv error: {:?}", e);
                    continue;
                }
            };

            let badge = utcb.get_badge();
            let proto = utcb.get_msg_tag().proto();
            let label = utcb.get_msg_tag().label();

            let res = self.dispatch(&mut utcb);
            if let Err(e) = res {
                if e == Error::Success {
                    continue;
                }
                error!(
                    "Failed to dispatch message for {}: {:?}, proto={:#x}, label={:#x}",
                    badge, e, proto, label
                );
                utcb.set_msg_tag(MsgTag::err());
                utcb.set_mr(0, e as usize);
            }

            if let Err(e) = self.reply(&mut utcb) {
                error!("Reply failed: {:?}", e);
            }
        }
        Ok(())
    }

    fn dispatch(&mut self, utcb: &mut UTCB) -> Result<(), Error> {
        let badge = utcb.get_badge();

        glenda::ipc_dispatch! {
            self, utcb,
            (protocol::KERNEL_PROTO, protocol::kernel::NOTIFY) => |s: &mut Self, u: &mut UTCB| {
                handle_notify(u, |_| {
                    let irq = badge.bits();
                    s.handle_irq(irq)
                })
            },
            (DEVICE_PROTO, device::REPORT) => |s: &mut Self, u: &mut UTCB| {
                handle_call(u, |u| {
                    let desc = unsafe { u.read_postcard()? };
                    s.report(badge, desc)
                })
            },
            (DEVICE_PROTO, device::UPDATE) => |s: &mut Self, u: &mut UTCB| {
                handle_call(u, |u| {
                    let compatible = unsafe { u.read_postcard()? };
                    s.update(badge, compatible)
                })
            },
            (DEVICE_PROTO, device::GET_MMIO) => |s: &mut Self, u: &mut UTCB| {
                handle_cap_call(u, |u| {
                    let id = u.get_mr(0) as usize;
                    let (frame, paddr, size) = s.get_mmio(badge, id, CapPtr::null())?;
                    u.set_mr(0, paddr);
                    u.set_mr(1, size);
                    Ok(frame.cap())
                })
            },
            (DEVICE_PROTO, device::GET_IRQ) => |s: &mut Self, u: &mut UTCB| {
                handle_cap_call(u, |u| {
                    let id = u.get_mr(0) as usize;
                    let handler = s.get_irq(badge, id, CapPtr::null())?;
                    Ok(handler.cap())
                })
            },
            (DEVICE_PROTO, device::HOOK) => |s: &mut Self, u: &mut UTCB| {
                handle_call(u, |u| {
                    let target = unsafe { u.read_postcard()? };
                    s.hook(badge, target, s.recv)
                })
            },
            (DEVICE_PROTO, device::UNHOOK) => |s: &mut Self, u: &mut UTCB| {
                handle_call(u, |u| {
                    let target = unsafe { u.read_postcard()? };
                    s.unhook(badge, target)
                })
            },
            (DEVICE_PROTO, device::SCAN_PLATFORM) => |s: &mut Self, u: &mut UTCB| {
                handle_call(u, |_| s.scan_platform(badge))
            },
            (DEVICE_PROTO, device::REGISTER_LOGIC) => |s: &mut Self, u: &mut UTCB| {
                handle_call(u, |u| {
                    let desc = unsafe { u.read_postcard()? };
                    s.register_logic(badge, desc, s.recv)
                })
            },
            (DEVICE_PROTO, device::ALLOC_LOGIC) => |s: &mut Self, u: &mut UTCB| {
                handle_cap_call(u, |u| {
                    let req: device::AllocLogicRequest = unsafe { u.read_postcard()? };
                    let ep = s.alloc_logic(badge, req.dev_type, &req.criteria,CapPtr::null())?;
                    Ok(ep.cap())
                })
            },
            (DEVICE_PROTO, device::QUERY) => |s: &mut Self, u: &mut UTCB| {
                handle_buffer_call(u, |u| {
                    let query = unsafe { u.read_postcard()? };
                    let names = s.query(badge, query)?;
                    unsafe { u.write_postcard(&names)? };
                    Ok(())
                })
            },
            (DEVICE_PROTO, device::GET_DESC) => |s: &mut Self, u: &mut UTCB| {
                handle_buffer_call(u, |u| {
                    let name = unsafe { u.read_str()? };
                    let desc = s.get_desc(badge, &name)?;
                    unsafe { u.write_postcard(&desc)? };
                    Ok(())
                })
            },
            (DEVICE_PROTO, device::GET_LOGIC_DESC) => |s: &mut Self, u: &mut UTCB| {
                handle_buffer_call(u, |u| {
                    let name = unsafe { u.read_str()? };
                    let (id, desc) = s.get_logic_desc(badge, &name)?;
                    u.set_mr(0, id as usize);
                    unsafe { u.write_postcard(&desc)? };
                    Ok(())
                })
            },
            (_, _) => |_,_| Err(Error::InvalidMethod)
        }
    }

    fn reply(&mut self, utcb: &mut UTCB) -> Result<(), Error> {
        self.reply.reply(utcb)
    }

    fn stop(&mut self) {
        self.running = false;
        self.init_client.report_service(Badge::null(), ServiceState::Stopped).unwrap_or_else(|e| {
            error!("Failed to report stopped state: {:?}", e);
        });
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
