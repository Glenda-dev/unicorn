extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

pub struct DriverEntry {
    pub compatible: String,
    pub binary: String,
}

pub struct Manifest {
    pub drivers: Vec<DriverEntry>,
}

impl Manifest {
    pub fn parse(data: &[u8]) -> Self {
        // Find null terminator or end of buffer
        let len = data.iter().position(|&c| c == 0).unwrap_or(data.len());
        let s = core::str::from_utf8(&data[..len]).unwrap_or("");
        let mut drivers = Vec::new();
        
        let mut current_compatible = None;
        let mut current_binary = None;
        
        for line in s.lines() {
            let line = line.trim();
            if line == "[[driver]]" {
                if let (Some(c), Some(b)) = (current_compatible.take(), current_binary.take()) {
                    drivers.push(DriverEntry { compatible: c, binary: b });
                }
            } else if line.starts_with("compatible") {
                if let Some(val) = parse_value(line) {
                    current_compatible = Some(val);
                }
            } else if line.starts_with("binary") {
                if let Some(val) = parse_value(line) {
                    current_binary = Some(val);
                }
            }
        }
        // Push last one
        if let (Some(c), Some(b)) = (current_compatible, current_binary) {
            drivers.push(DriverEntry { compatible: c, binary: b });
        }
        
        Self { drivers }
    }
    
    pub fn find_binary(&self, compatible: &str) -> Option<&str> {
        for driver in &self.drivers {
            if driver.compatible == compatible {
                return Some(&driver.binary);
            }
        }
        None
    }
}

fn parse_value(line: &str) -> Option<String> {
    let parts: Vec<&str> = line.split('=').collect();
    if parts.len() != 2 { return None; }
    let val = parts[1].trim();
    if val.starts_with('"') && val.ends_with('"') {
        Some(val[1..val.len()-1].into())
    } else {
        None
    }
}
