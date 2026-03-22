use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::platform::ProcessHandle;
use crate::scan::value_type::{ScanValue, ValueType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressEntry {
    pub address: usize,
    pub value_type: ValueType,
    pub description: String,
    #[serde(skip)]
    pub current_value: Option<ScanValue>,
    pub frozen: bool,
    pub frozen_value: Option<ScanValue>,
}

impl AddressEntry {
    pub fn new(address: usize, value_type: ValueType, description: String) -> Self {
        Self {
            address,
            value_type,
            description,
            current_value: None,
            frozen: false,
            frozen_value: None,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AddressTable {
    pub entries: Vec<AddressEntry>,
}

impl AddressTable {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn add(&mut self, entry: AddressEntry) {
        self.entries.push(entry);
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.entries.len() {
            self.entries.remove(index);
        }
    }

    pub fn toggle_freeze(&mut self, index: usize) {
        if let Some(entry) = self.entries.get_mut(index) {
            entry.frozen = !entry.frozen;
            if entry.frozen {
                entry.frozen_value = entry.current_value.clone();
            } else {
                entry.frozen_value = None;
            }
        }
    }

    pub fn update_values(&mut self, handle: &dyn ProcessHandle) {
        for entry in &mut self.entries {
            if let Ok(data) = handle.read_memory(entry.address, entry.value_type.size()) {
                entry.current_value = ScanValue::from_bytes(&data, entry.value_type);
            }
        }
    }

    pub fn write_frozen_values(&self, handle: &dyn ProcessHandle) {
        for entry in &self.entries {
            if entry.frozen {
                if let Some(ref val) = entry.frozen_value {
                    let _ = handle.write_memory(entry.address, &val.to_bytes());
                }
            }
        }
    }

    pub fn write_value(&mut self, handle: &dyn ProcessHandle, index: usize, value: ScanValue) -> Result<()> {
        if let Some(entry) = self.entries.get_mut(index) {
            handle.write_memory(entry.address, &value.to_bytes())?;
            entry.current_value = Some(value.clone());
            if entry.frozen {
                entry.frozen_value = Some(value);
            }
        }
        Ok(())
    }

    pub fn save_to_file(&self, path: &str) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn load_from_file(path: &str) -> Result<Self> {
        let json = std::fs::read_to_string(path)?;
        let table: Self = serde_json::from_str(&json)?;
        Ok(table)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_remove() {
        let mut table = AddressTable::new();
        table.add(AddressEntry::new(0x1000, ValueType::U32, "health".into()));
        table.add(AddressEntry::new(0x2000, ValueType::F32, "speed".into()));
        assert_eq!(table.entries.len(), 2);

        table.remove(0);
        assert_eq!(table.entries.len(), 1);
        assert_eq!(table.entries[0].address, 0x2000);
    }

    #[test]
    fn test_toggle_freeze() {
        let mut table = AddressTable::new();
        let mut entry = AddressEntry::new(0x1000, ValueType::U32, "test".into());
        entry.current_value = Some(ScanValue::U32(100));
        table.add(entry);

        table.toggle_freeze(0);
        assert!(table.entries[0].frozen);
        assert_eq!(table.entries[0].frozen_value, Some(ScanValue::U32(100)));

        table.toggle_freeze(0);
        assert!(!table.entries[0].frozen);
        assert!(table.entries[0].frozen_value.is_none());
    }

    #[test]
    fn test_serialize_deserialize() {
        let mut table = AddressTable::new();
        table.add(AddressEntry::new(0x1000, ValueType::U32, "health".into()));
        table.add(AddressEntry::new(0x2000, ValueType::F32, "speed".into()));
        table.entries[0].frozen = true;
        table.entries[0].frozen_value = Some(ScanValue::U32(999));

        let json = serde_json::to_string(&table).unwrap();
        let restored: AddressTable = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.entries.len(), 2);
        assert_eq!(restored.entries[0].address, 0x1000);
        assert_eq!(restored.entries[0].description, "health");
        assert!(restored.entries[0].frozen);
        assert_eq!(restored.entries[1].value_type, ValueType::F32);
    }

    #[test]
    fn test_save_load_file() {
        let mut table = AddressTable::new();
        table.add(AddressEntry::new(0xDEAD, ValueType::U64, "test_entry".into()));

        let path = std::env::temp_dir().join("cheat_test_table.json");
        let path_str = path.to_str().unwrap();

        table.save_to_file(path_str).unwrap();
        let loaded = AddressTable::load_from_file(path_str).unwrap();

        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].address, 0xDEAD);
        assert_eq!(loaded.entries[0].description, "test_entry");

        std::fs::remove_file(path).ok();
    }
}
