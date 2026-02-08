use alloc::string::String;
use glenda::error::Error;
use glenda::interface::DeviceService;
use glenda::ipc::Badge;
use glenda::protocol::device::DeviceNode;
use glenda::utils::PlatformInfo;

pub struct DeviceManager {
    // Basic Device manager
}

impl DeviceManager {
    pub fn new() -> Self {
        Self {}
    }
}

impl DeviceService for DeviceManager {
    fn scan_platform(&mut self, _badge: Badge, _info: &PlatformInfo) -> Result<(), Error> {
        unimplemented!()
    }
    fn find_compatible(&self, _badge: Badge, _compat: String) -> Result<DeviceNode, Error> {
        unimplemented!()
    }
}
