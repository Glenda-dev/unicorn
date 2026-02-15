use crate::unicorn::UnicornManager;
use alloc::vec::Vec;
use glenda::cap::{Frame, IrqHandler};
use glenda::error::Error;
use glenda::interface::DeviceService;
use glenda::ipc::Badge;
use glenda::protocol::device::DeviceDescNode;

impl<'a> DeviceService for UnicornManager<'a> {
    fn scan_platform(&mut self, _badge: Badge) -> Result<(), Error> {
        unimplemented!()
    }

    fn get_mmio(&mut self, _badge: Badge, _id: usize) -> Result<(Frame, usize, usize), Error> {
        unimplemented!()
    }

    fn get_irq(&mut self, _badge: Badge, _id: usize) -> Result<IrqHandler, Error> {
        unimplemented!()
    }

    fn report(&mut self, badge: Badge, desc: Vec<DeviceDescNode>) -> Result<(), Error> {
        let driver_id = badge.bits();
        if let Some(parent_id) = self.pids.get(&driver_id) {
            self.tree.mount_subtree(*parent_id, desc)
        } else {
            // If the driver is not registered, we cannot attach the subtree.
            // For the root platform driver (if any), we might have a special logic,
            // but usually it should be registered too.
            Err(Error::InvalidArgs)
        }
    }
}
