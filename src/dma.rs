use glenda::error::Error;
use glenda::manager::interface::IDmaService;

pub struct DmaManager {
    // Basic DMA manager
}

impl DmaManager {
    pub fn new() -> Self {
        Self {}
    }
}

impl IDmaService for DmaManager {
    fn alloc_dma(&mut self, _size: usize) -> Result<usize, Error> {
        // TODO: Implement DMA allocation (physically contiguous)
        Err(Error::NotSupported)
    }

    fn free_dma(&mut self, _paddr: usize, _size: usize) {
        // TODO: Implement DMA free
    }
}
