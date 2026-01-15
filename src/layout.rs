use glenda::cap::{CapPtr, Endpoint, Frame};

pub const FACTOTUM_SLOT: usize = 10;
pub const DTB_SLOT: usize = 11;
pub const UNICORN_ENDPOINT_SLOT: usize = 12;
pub const MANIFEST_ADDR: usize = 0x2000_0000;
pub const INITRD_VA: usize = 0x4000_0000;

pub const FACTOTUM_CAP: Endpoint = Endpoint::from(CapPtr::from(FACTOTUM_SLOT));
pub const DTB_CAP: Frame = Frame::from(CapPtr::from(DTB_SLOT));
pub const UNICORN_ENDPOINT_CAP: Endpoint = Endpoint::from(CapPtr::from(UNICORN_ENDPOINT_SLOT));
