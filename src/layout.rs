use glenda::cap::{CapPtr, Endpoint, Frame};

pub const FACTOTUM_SLOT: usize = 10;
pub const PLATFORM_SLOT: usize = 6;
pub const UNTYPED_SLOT: usize = 7;
pub const MMIO_SLOT: usize = 8;
pub const IRQ_SLOT: usize = 9;
pub const UNICORN_ENDPOINT_SLOT: usize = 11;

pub const FACTOTUM_CAP: Endpoint = Endpoint::from(CapPtr::from(FACTOTUM_SLOT));
pub const PLATFORM_CAP: Frame = Frame::from(CapPtr::from(PLATFORM_SLOT));
pub const UNICORN_ENDPOINT_CAP: Endpoint = Endpoint::from(CapPtr::from(UNICORN_ENDPOINT_SLOT));
