use super::DeviceState;
use super::platform::DeviceId;
use crate::layout::{IRQ_CONTROL_CAP, KERNEL_CAP};
use crate::unicorn::UnicornManager;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use glenda::arch::mem::PGSIZE;
use glenda::cap::{CapPtr, Endpoint, Frame, IrqHandler};
use glenda::error::Error;
use glenda::interface::DeviceService;
use glenda::ipc::Badge;
use glenda::protocol::device::{self, DeviceDescNode, HookTarget, LogicDeviceDesc, NOTIFY_HOOK};
use glenda::utils::manager::CSpaceService;

impl<'a> UnicornManager<'a> {
    fn scan_subtree(&mut self, start_id: DeviceId) -> Result<(), Error> {
        let mut queue = VecDeque::new();
        queue.push_back(start_id);

        while let Some(id) = queue.pop_front() {
            let (needs_start, children) = if let Some(node) = self.tree.get_node(id) {
                (node.state == DeviceState::Ready, node.children.clone())
            } else {
                (false, alloc::vec![])
            };

            if needs_start {
                self.spawn_queue.push_back(id);
            }

            for child in children {
                queue.push_back(child);
            }
        }
        Ok(())
    }

    fn notify_hook_on_logic(
        &self,
        logic_id: usize,
        hooks: &[(HookTarget, CapPtr)],
    ) -> Result<(), Error> {
        let (desc, ep, name) =
            self.logic_service.devices.get(&logic_id).cloned().ok_or(Error::NotFound)?;

        let mut notify_eps = Vec::new();
        for (target, hook_ep) in hooks {
            let notify = match target {
                HookTarget::Endpoint(e) => *e == ep.bits() as u64,
                HookTarget::Type(t) => *t == desc.dev_type,
            };
            if notify {
                notify_eps.push(*hook_ep);
            }
        }

        for hook_ep in notify_eps {
            log!("Notifying hook {:?} for logic device {}", hook_ep, name);
            Endpoint::from(hook_ep).notify(Badge::new(NOTIFY_HOOK))?;
        }
        Ok(())
    }

    fn find_node_by_name(&self, name: &str) -> Option<DeviceId> {
        if let Some(root) = self.tree.root {
            let mut queue = VecDeque::new();
            queue.push_back(root);
            while let Some(id) = queue.pop_front() {
                if let Some(node) = self.tree.get_node(id) {
                    if node.desc.name == name {
                        return Some(id);
                    }
                    for child in &node.children {
                        queue.push_back(*child);
                    }
                }
            }
        }
        None
    }
}

impl<'a> DeviceService for UnicornManager<'a> {
    fn scan_platform(&mut self, _badge: Badge) -> Result<(), Error> {
        if let Some(root) = self.tree.root { self.scan_subtree(root) } else { Ok(()) }
    }

    fn get_mmio(
        &mut self,
        badge: Badge,
        id: usize,
        _recv: CapPtr,
    ) -> Result<(Frame, usize, usize), Error> {
        let driver_id = badge.bits();
        let &node_id = self.pids.get(&driver_id).ok_or(Error::InvalidArgs)?;

        let (base_addr, size, name) = {
            let node = self.tree.get_node(node_id).ok_or(Error::InvalidArgs)?;
            if id >= node.desc.mmio.len() {
                return Err(Error::InvalidArgs);
            }
            let region = &node.desc.mmio[id];
            (region.base_addr, region.size, node.desc.name.clone())
        };

        if let Some(&slot) = self.mmio_caps.get(&base_addr) {
            log!("Using cached MMIO region for driver {}: base={:#x}", driver_id, base_addr);
            return Ok((Frame::from(slot), base_addr, size));
        }

        let slot = self.cspace_mgr.alloc(self.res_client)?;
        let pages = (size + PGSIZE - 1) / PGSIZE;
        KERNEL_CAP.get_mmio(base_addr, pages, slot)?;
        self.mmio_caps.insert(base_addr, slot);
        log!(
            "Provided MMIO region for driver {}: base={:#x}, size={:#x}, name={}",
            driver_id,
            base_addr,
            size,
            name
        );
        Ok((Frame::from(slot), base_addr, size))
    }

    fn get_irq(&mut self, badge: Badge, id: usize, _recv: CapPtr) -> Result<IrqHandler, Error> {
        let driver_id = badge.bits();
        let &node_id = self.pids.get(&driver_id).ok_or(Error::InvalidArgs)?;

        let irq_num = {
            let node = self.tree.get_node(node_id).ok_or(Error::InvalidArgs)?;
            if id >= node.desc.irq.len() {
                return Err(Error::InvalidArgs);
            }
            node.desc.irq[id]
        };

        if let Some(&slot) = self.irq_caps.get(&irq_num) {
            log!("Using cached IRQ for driver {}: irq_num={}", driver_id, irq_num);
            return Ok(IrqHandler::from(slot));
        }

        let slot = self.cspace_mgr.alloc(self.res_client)?;
        // Get IRQ via Kernel Cap
        KERNEL_CAP.get_irq(irq_num, slot)?;

        let handler = IrqHandler::from(slot);
        // "unicorn在授权时设置优先级以打开中断"
        IRQ_CONTROL_CAP.set_priority(irq_num, 1)?;

        self.irq_caps.insert(irq_num, slot);
        log!("Provided IRQ for driver {}: irq_num={}, slot={:?}", driver_id, irq_num, slot);
        Ok(handler)
    }

    fn report(&mut self, badge: Badge, desc: Vec<DeviceDescNode>) -> Result<(), Error> {
        let driver_id = badge.bits();
        if let Some(&node_id) = self.pids.get(&driver_id) {
            self.tree.mount_subtree(node_id, desc)?;
            self.scan_subtree(node_id)
        } else {
            Err(Error::InvalidArgs)
        }
    }

    fn update(
        &mut self,
        badge: Badge,
        compatible: Vec<alloc::string::String>,
    ) -> Result<(), Error> {
        let driver_id = badge.bits();
        if let Some(node_id) = self.pids.remove(&driver_id) {
            {
                let node = self.tree.get_node_mut(node_id).ok_or(Error::InvalidArgs)?;
                node.desc.compatible = compatible;
                node.state = super::DeviceState::Ready;
            }
            self.scan_subtree(node_id)
        } else {
            Err(Error::InvalidArgs)
        }
    }

    fn register_logic(
        &mut self,
        _badge: Badge,
        desc: LogicDeviceDesc,
        endpoint: CapPtr,
    ) -> Result<(), Error> {
        let (id, _name, _ep) = self.logic_service.register(
            self.cspace_mgr,
            self.res_client,
            desc.clone(),
            endpoint,
        )?;

        if let Some(node_id) = self.find_node_by_name(&desc.parent_name) {
            if let Some(node) = self.tree.get_node_mut(node_id) {
                node.logical_devices.push(id);
            }
        }

        self.notify_hook_on_logic(id, &self.hooks)
    }

    fn alloc_logic(
        &mut self,
        badge: Badge,
        dev_type: device::LogicDeviceType,
        criteria: &str,
        _recv: CapPtr,
    ) -> Result<Endpoint, Error> {
        self.logic_service.alloc(self.cspace_mgr, self.res_client, badge, dev_type, criteria)
    }

    fn query(
        &mut self,
        _badge: Badge,
        query: device::DeviceQuery,
    ) -> Result<Vec<alloc::string::String>, Error> {
        log!(
            "Querying devices with criteria: name={:?}, compatible={:?}, dev_type={:?}",
            query.name,
            query.compatible,
            query.dev_type
        );
        self.logic_service.query(query)
    }

    fn get_desc(&mut self, _badge: Badge, name: &str) -> Result<device::DeviceDesc, Error> {
        if let Some(root) = self.tree.root {
            let mut queue = VecDeque::new();
            queue.push_back(root);
            while let Some(id) = queue.pop_front() {
                if let Some(node) = self.tree.get_node(id) {
                    if node.desc.name == name {
                        return Ok(node.desc.clone());
                    }
                    for child in &node.children {
                        queue.push_back(*child);
                    }
                }
            }
        }
        Err(Error::NotFound)
    }

    fn get_logic_desc(
        &mut self,
        _badge: Badge,
        name: &str,
    ) -> Result<(u64, LogicDeviceDesc), Error> {
        let (id, desc) = self.logic_service.get_desc(name).ok_or(Error::NotFound)?;
        Ok((id as u64, desc))
    }

    fn hook(&mut self, _badge: Badge, target: HookTarget, endpoint: CapPtr) -> Result<(), Error> {
        let slot = self.cspace_mgr.alloc(self.res_client)?;
        self.cspace_mgr.root().move_cap(endpoint, slot)?;
        log!("Registering hook for target {:?} at endpoint {:?}", target, slot);
        let new_hook = (target, slot);
        self.hooks.push(new_hook);
        Endpoint::from(slot).notify(Badge::new(NOTIFY_HOOK))?;
        Ok(())
    }

    fn unhook(&mut self, _badge: Badge, _target: HookTarget) -> Result<(), Error> {
        Err(Error::NotImplemented)
    }
}
