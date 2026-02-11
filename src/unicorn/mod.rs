use crate::config::Manifest;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use glenda::cap::{CapPtr, Endpoint, Reply};
use glenda::client::ProcessClient;
use glenda::client::ResourceClient;
use glenda::protocol::device::DeviceNode;
use glenda::utils::manager::CSpaceManager;
use glenda::utils::platform::PlatformInfo;

pub mod device;
pub mod dma;
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
    pub nodes: Vec<DeviceNode>,
    pub pids: BTreeMap<usize, usize>,
    pub platform: Option<Box<PlatformInfo>>,
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
            nodes: Vec::new(),
            pids: BTreeMap::new(),
            platform: None,
        }
    }
}
