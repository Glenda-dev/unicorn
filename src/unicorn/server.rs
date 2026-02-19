use crate::UnicornManager;
use crate::layout::{
    BOOTINFO_ADDR, BOOTINFO_SLOT, IRQ_SLOT, MANIFEST_SLOT, MMIO_SLOT, RESOURCE_ADDR,
};
use crate::{error, log};
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
                let badge = utcb.get_badge();
                error!("Failed to dispatch message for {}: {:?}", badge, e);
                utcb.set_msg_tag(MsgTag::err());
                utcb.set_mr(0, e as usize);
            }

            self.reply(&mut utcb)?;
        }
        Ok(())
    }

    fn dispatch(&mut self, utcb: &mut UTCB) -> Result<(), Error> {
        let badge = utcb.get_badge();

        glenda::ipc_dispatch! {
            self, utcb,
            (protocol::KERNEL_PROTO, protocol::kernel::NOTIFY) => |s: &mut Self, _u: &mut UTCB| {
                let irq = badge.bits();
                s.handle_irq(irq)
            },
            (glenda_drivers::protocol::BLOCK_PROTO, glenda_drivers::protocol::block::GET_CAPACITY) => |s: &mut Self, u: &mut UTCB| {
                 handle_call(u, |_| {
                     let (desc, _, _) = s.logical_devices.get(&badge.bits()).ok_or(Error::NotFound)?;
                     if let glenda::protocol::device::LogicDeviceType::Block(ref meta) = desc.dev_type {
                         Ok(meta.num_blocks as usize)
                     } else {
                         Err(Error::InvalidArgs)
                     }
                 })
            },
            (glenda_drivers::protocol::BLOCK_PROTO, glenda_drivers::protocol::block::GET_BLOCK_SIZE) => |s: &mut Self, u: &mut UTCB| {
                 handle_call(u, |_| {
                     let (desc, _, _) = s.logical_devices.get(&badge.bits()).ok_or(Error::NotFound)?;
                     if let glenda::protocol::device::LogicDeviceType::Block(ref meta) = desc.dev_type {
                         Ok(meta.block_size as usize)
                     } else {
                         Ok(512)
                     }
                 })
            },
            (glenda_drivers::protocol::BLOCK_PROTO, glenda_drivers::protocol::block::SETUP_RING) => |s: &mut Self, u: &mut UTCB| {
                handle_cap_call(u, |_u| {
                    let (desc, _, name) = s.logical_devices.get(&badge.bits()).ok_or(Error::NotFound)?;
                    if let glenda::protocol::device::LogicDeviceType::Block(ref _meta) = desc.dev_type {
                         // Proxy ring logic:
                         // 1. Get raw block endpoint
                         // 2. Wrap as BlockClient
                         // 3. setup_ring on raw block
                         // 4. Return that frame to consumer

                         // Note: In a complete implementation, Unicorn would need to intercept the
                         // SQEs on this ring to add the partition offset.
                         // This requires a background task or polling the ring.
                         log!("Proxying io_uring for partition '{}'", name);
                         Err(Error::NotImplemented)
                    } else {
                         Err(Error::NotFound)
                    }
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
                handle_call(u, |_| s.scan_platform(badge))            },
            (DEVICE_PROTO, device::REGISTER_LOGIC) => |s: &mut Self, u: &mut UTCB| {
                handle_call(u, |u| {
                    let desc = unsafe { u.read_postcard()? };
                    let tag = u.get_msg_tag();
                    let endpoint = if tag.flags().contains(glenda::ipc::MsgFlags::HAS_CAP) {
                        u.get_recv_window()
                    } else {
                        glenda::cap::CapPtr::null()
                    };
                    s.register_logic(badge, desc, endpoint)
                })
            },
            (DEVICE_PROTO, device::ALLOC_LOGIC) => |s: &mut Self, u: &mut UTCB| {
                handle_cap_call(u, |u| {
                    let (dev_type, criteria): (u32, alloc::string::String) =
                        unsafe { u.read_postcard()? };
                    let ep = s.alloc_logic(badge, dev_type, &criteria)?;
                    Ok(ep.cap())
                })
            },
            (DEVICE_PROTO, device::QUERY) => |s: &mut Self, u: &mut UTCB| {
                handle_call(u, |u| {
                    let query = unsafe { u.read_postcard()? };
                    let names = s.query(badge, query)?;
                    unsafe { u.write_postcard(&names)? };
                    u.set_msg_tag(glenda::ipc::MsgTag::new(0, 0, glenda::ipc::MsgFlags::HAS_BUFFER));
                    Ok(())
                })
            },
            (DEVICE_PROTO, device::GET_DESC) => |s: &mut Self, u: &mut UTCB| {
                handle_call(u, |u| {
                    let name: alloc::string::String = unsafe { u.read_postcard()? };
                    let desc = s.get_desc(badge, &name)?;
                    unsafe { u.write_postcard(&desc)? };
                    u.set_msg_tag(glenda::ipc::MsgTag::new(0, 0, glenda::ipc::MsgFlags::HAS_BUFFER));
                    Ok(())
                })
            }
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
