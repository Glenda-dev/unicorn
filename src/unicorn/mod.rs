use crate::config::Manifest;
use crate::unicorn::platform::{DeviceId, DeviceTree};
use alloc::collections::BTreeMap;
use glenda::cap::{CapPtr, Endpoint, Reply};
use glenda::client::ProcessClient;
use glenda::client::ResourceClient;
use glenda::utils::manager::CSpaceManager;

pub mod device;
pub mod platform;
pub mod server;

pub struct UnicornManager<'a> {
    pub running: bool,
    pub endpoint: Endpoint,
    pub reply: Reply,
    pub recv: CapPtr,
    pub cspace_mgr: &'a mut CSpaceManager,
    pub res_client: &'a mut ResourceClient,
    pub proc_client: &'a mut ProcessClient,
    pub config: Manifest,
    pub tree: DeviceTree,
    pub pids: BTreeMap<usize, DeviceId>, // driver_badge -> node_id
    pub irqs: BTreeMap<usize, DeviceId>, // irq_num -> node_id
    pub irq_caps: BTreeMap<usize, CapPtr>,
    pub mmio_caps: BTreeMap<usize, CapPtr>, // base_addr -> slot
}

impl<'a> UnicornManager<'a> {
    pub fn new(
        cspace_mgr: &'a mut CSpaceManager,
        res_client: &'a mut ResourceClient,
        proc_client: &'a mut ProcessClient,
    ) -> Self {
        Self {
            running: false,
            endpoint: Endpoint::from(CapPtr::null()),
            reply: Reply::from(CapPtr::null()),
            recv: CapPtr::null(),
            cspace_mgr,
            res_client,
            proc_client,
            config: Manifest::new(),
            tree: DeviceTree::new(),
            pids: BTreeMap::new(),
            irqs: BTreeMap::new(),
            irq_caps: BTreeMap::new(),
            mmio_caps: BTreeMap::new(),
        }
    }
}
