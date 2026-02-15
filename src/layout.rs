use glenda::cap::{CapPtr, Frame, IrqHandler, Mmio};

pub const BOOTINFO_SLOT: CapPtr = CapPtr::from(9);
pub const MMIO_SLOT: CapPtr = CapPtr::from(11);
pub const IRQ_SLOT: CapPtr = CapPtr::from(12);
pub const MMIO_CAP: Mmio = Mmio::from(MMIO_SLOT);
pub const IRQ_CAP: IrqHandler = IrqHandler::from(IRQ_SLOT);
pub const MANIFEST_SLOT: CapPtr = CapPtr::from(15);
pub const MANIFEST_CAP: Frame = Frame::from(MANIFEST_SLOT);

pub const RESOURCE_ADDR: usize = 0x3000_0000;
pub const BOOTINFO_ADDR: usize = 0x3100_0000;
