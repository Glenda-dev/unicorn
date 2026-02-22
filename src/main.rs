#![no_std]
#![no_main]
#![allow(dead_code)]

#[macro_use]
extern crate glenda;

extern crate alloc;
mod config;
mod layout;
mod unicorn;

use glenda::cap::CapType;
use glenda::cap::{CSPACE_CAP, ENDPOINT_CAP, ENDPOINT_SLOT, MONITOR_CAP, RECV_SLOT, REPLY_SLOT};
use glenda::client::ResourceClient;
use glenda::client::{InitClient, ProcessClient};
use glenda::error::Error;
use glenda::interface::{ResourceService, SystemService};
use glenda::ipc::Badge;
use glenda::protocol::resource::{INIT_ENDPOINT, ResourceType};
use glenda::utils::manager::CSpaceManager;
use layout::{INIT_CAP, INIT_SLOT};
use unicorn::UnicornManager;

#[unsafe(no_mangle)]
fn main() -> usize {
    glenda::console::init_logging("Unicorn");
    log!("Starting Unicorn Device Driver Manager...");

    let mut res_client = ResourceClient::new(MONITOR_CAP);
    let mut proc_client = ProcessClient::new(MONITOR_CAP);
    res_client
        .alloc(Badge::null(), CapType::Endpoint, 0, ENDPOINT_SLOT)
        .expect("Failed to allocate endpoint cap for unicorn");
    res_client
        .get_cap(Badge::null(), ResourceType::Endpoint, INIT_ENDPOINT, INIT_SLOT)
        .expect("Failed to get init endpoint");
    let mut cspace_mgr = CSpaceManager::new(CSPACE_CAP, 16);
    let mut init_client = InitClient::new(INIT_CAP);
    let mut server =
        UnicornManager::new(&mut cspace_mgr, &mut res_client, &mut proc_client, &mut init_client);
    if let Err(e) = load_unicorn(&mut server) {
        log!("Failed to load: {:?}", e);
        return 1;
    }
    server.run().expect("Unicorn exited");
    1
}

fn load_unicorn(server: &mut UnicornManager) -> Result<(), Error> {
    server.listen(ENDPOINT_CAP, REPLY_SLOT, RECV_SLOT)?;
    server.init()?;
    Ok(())
}
