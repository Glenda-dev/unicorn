use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Manifest {
    pub drivers: Vec<DriverEntry>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DriverEntry {
    pub name: String,
    pub compatible: Vec<String>,
}

impl Manifest {
    pub const fn new() -> Self {
        Self { drivers: Vec::new() }
    }
}
