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
    /// Set when the most recent freeze write failed (stale address, unmapped
    /// page, detached process). Runtime state only - never persisted.
    #[serde(skip)]
    pub freeze_error: bool,
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
            freeze_error: false,
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

    /// Toggle freeze on an entry. When enabling, the lock value is snapshotted
    /// straight from process memory so freezing works before the first periodic
    /// value refresh; the last known value is used as a fallback. If no value
    /// can be obtained the entry stays unfrozen and an error is returned,
    /// rather than silently freezing to nothing.
    pub fn toggle_freeze(&mut self, index: usize, handle: Option<&dyn ProcessHandle>) -> Result<()> {
        let Some(entry) = self.entries.get_mut(index) else {
            return Ok(());
        };

        if entry.frozen {
            entry.frozen = false;
            entry.frozen_value = None;
            entry.freeze_error = false;
            return Ok(());
        }

        let snapshot = handle
            .and_then(|h| h.read_memory(entry.address, entry.value_type.size()).ok())
            .and_then(|data| ScanValue::from_bytes(&data, entry.value_type))
            .or_else(|| entry.current_value.clone());

        let Some(value) = snapshot else {
            return Err(anyhow::anyhow!(
                "Cannot freeze 0x{:X}: value is unreadable",
                entry.address
            ));
        };

        entry.current_value = Some(value.clone());
        entry.frozen_value = Some(value);
        entry.frozen = true;
        entry.freeze_error = false;
        Ok(())
    }

    pub fn update_values(&mut self, handle: &dyn ProcessHandle) {
        for entry in &mut self.entries {
            if let Ok(data) = handle.read_memory(entry.address, entry.value_type.size()) {
                entry.current_value = ScanValue::from_bytes(&data, entry.value_type);
            }
        }
    }

    /// Drop every freeze lock while keeping the rows. Used when the entries can
    /// no longer be trusted to point at the process they were captured from -
    /// otherwise the freeze loop would hammer absolute addresses in whatever
    /// process is attached now.
    pub fn clear_freezes(&mut self) {
        for entry in &mut self.entries {
            entry.frozen = false;
            entry.frozen_value = None;
            entry.freeze_error = false;
            entry.current_value = None;
        }
    }

    pub fn has_frozen(&self) -> bool {
        self.entries.iter().any(|e| e.frozen)
    }

    pub fn freeze_error_count(&self) -> usize {
        self.entries.iter().filter(|e| e.freeze_error).count()
    }

    /// Re-write every frozen value. Called on a short interval - failures are
    /// recorded per entry instead of raised, so a dead address shows up as a
    /// marker in the table rather than flooding the error banner.
    pub fn write_frozen_values(&mut self, handle: &dyn ProcessHandle) {
        for entry in &mut self.entries {
            if !entry.frozen {
                entry.freeze_error = false;
                continue;
            }
            entry.freeze_error = match entry.frozen_value {
                Some(ref val) => handle.write_memory(entry.address, &val.to_bytes()).is_err(),
                None => true,
            };
        }
    }

    /// Like [`Self::write_frozen_values`], but refuses to write to an address
    /// that has fallen outside `writable`, a sorted list of `(start, end)`
    /// ranges.
    ///
    /// Cross-process a stale address merely fails. In-process - which is what
    /// the Android overlay is - the allocator may have handed that page to
    /// something else, and an unguarded freeze loop then corrupts unrelated
    /// live state ten times a second. The entry is marked in error instead.
    pub fn write_frozen_values_within(
        &mut self,
        handle: &dyn ProcessHandle,
        writable: &[(usize, usize)],
    ) {
        for entry in &mut self.entries {
            if !entry.frozen {
                entry.freeze_error = false;
                continue;
            }
            let Some(ref val) = entry.frozen_value else {
                entry.freeze_error = true;
                continue;
            };
            let bytes = val.to_bytes();
            let end = entry.address.saturating_add(bytes.len());
            // The last range starting at or before this address is the only
            // one that can contain it.
            let idx = writable.partition_point(|(start, _)| *start <= entry.address);
            let mapped = idx > 0 && end <= writable[idx - 1].1;

            entry.freeze_error =
                !mapped || handle.write_memory(entry.address, &bytes).is_err();
        }
    }

    pub fn write_value(&mut self, handle: &dyn ProcessHandle, index: usize, value: ScanValue) -> Result<()> {
        if let Some(entry) = self.entries.get_mut(index) {
            handle.write_memory(entry.address, &value.to_bytes())?;
            entry.current_value = Some(value.clone());
            if entry.frozen {
                entry.frozen_value = Some(value);
                entry.freeze_error = false;
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
    use crate::platform::MemoryRegion;
    use std::sync::Mutex;

    struct MockHandle {
        // A `Mutex`, not a `RefCell`: `ProcessHandle` is `Sync` so the real
        // handles can be shared across the scan and freeze threads, and the
        // test double has to satisfy the same bound.
        memory: Mutex<Vec<u8>>,
        base: usize,
        writable: bool,
    }

    impl MockHandle {
        fn new(base: usize, memory: Vec<u8>) -> Self {
            Self {
                memory: Mutex::new(memory),
                base,
                writable: true,
            }
        }
    }

    impl ProcessHandle for MockHandle {
        fn read_memory(&self, address: usize, size: usize) -> Result<Vec<u8>> {
            let offset = address
                .checked_sub(self.base)
                .ok_or_else(|| anyhow::anyhow!("out of range"))?;
            let mem = self.memory.lock().unwrap();
            if offset + size > mem.len() {
                return Err(anyhow::anyhow!("out of range"));
            }
            Ok(mem[offset..offset + size].to_vec())
        }

        fn write_memory(&self, address: usize, data: &[u8]) -> Result<()> {
            if !self.writable {
                return Err(anyhow::anyhow!("write denied"));
            }
            let offset = address
                .checked_sub(self.base)
                .ok_or_else(|| anyhow::anyhow!("out of range"))?;
            let mut mem = self.memory.lock().unwrap();
            if offset + data.len() > mem.len() {
                return Err(anyhow::anyhow!("out of range"));
            }
            mem[offset..offset + data.len()].copy_from_slice(data);
            Ok(())
        }

        fn memory_regions(&self) -> Result<Vec<MemoryRegion>> {
            Ok(Vec::new())
        }
    }

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

        table.toggle_freeze(0, None).unwrap();
        assert!(table.entries[0].frozen);
        assert_eq!(table.entries[0].frozen_value, Some(ScanValue::U32(100)));

        table.toggle_freeze(0, None).unwrap();
        assert!(!table.entries[0].frozen);
        assert!(table.entries[0].frozen_value.is_none());
    }

    #[test]
    fn test_toggle_freeze_snapshots_from_memory() {
        let handle = MockHandle::new(0x1000, 42u32.to_le_bytes().to_vec());
        let mut table = AddressTable::new();
        // No current_value yet - the periodic refresh has not run.
        table.add(AddressEntry::new(0x1000, ValueType::U32, "test".into()));

        table.toggle_freeze(0, Some(&handle)).unwrap();
        assert!(table.entries[0].frozen);
        assert_eq!(table.entries[0].frozen_value, Some(ScanValue::U32(42)));
        assert_eq!(table.entries[0].current_value, Some(ScanValue::U32(42)));
    }

    #[test]
    fn test_toggle_freeze_without_value_stays_unfrozen() {
        let mut table = AddressTable::new();
        table.add(AddressEntry::new(0x1000, ValueType::U32, "test".into()));

        // No handle and no cached value - must not silently freeze to nothing.
        assert!(table.toggle_freeze(0, None).is_err());
        assert!(!table.entries[0].frozen);
        assert!(table.entries[0].frozen_value.is_none());

        // Unreadable address behaves the same way.
        let handle = MockHandle::new(0x9000, vec![0; 4]);
        assert!(table.toggle_freeze(0, Some(&handle)).is_err());
        assert!(!table.entries[0].frozen);
    }

    #[test]
    fn test_write_frozen_values_rewrites_memory() {
        let handle = MockHandle::new(0x1000, 100u32.to_le_bytes().to_vec());
        let mut table = AddressTable::new();
        table.add(AddressEntry::new(0x1000, ValueType::U32, "test".into()));
        table.toggle_freeze(0, Some(&handle)).unwrap();

        // Simulate the target process overwriting the value.
        handle.write_memory(0x1000, &7u32.to_le_bytes()).unwrap();
        table.write_frozen_values(&handle);

        assert_eq!(handle.read_memory(0x1000, 4).unwrap(), 100u32.to_le_bytes());
        assert!(!table.entries[0].freeze_error);
        assert_eq!(table.freeze_error_count(), 0);
    }

    #[test]
    fn test_freeze_error_is_set_and_cleared() {
        let mut handle = MockHandle::new(0x1000, 100u32.to_le_bytes().to_vec());
        let mut table = AddressTable::new();
        table.add(AddressEntry::new(0x1000, ValueType::U32, "test".into()));
        table.toggle_freeze(0, Some(&handle)).unwrap();

        handle.writable = false;
        table.write_frozen_values(&handle);
        assert!(table.entries[0].freeze_error);
        assert_eq!(table.freeze_error_count(), 1);

        // A transient failure must not latch the marker forever.
        handle.writable = true;
        table.write_frozen_values(&handle);
        assert!(!table.entries[0].freeze_error);

        // Unfreezing clears it too.
        handle.writable = false;
        table.write_frozen_values(&handle);
        assert!(table.entries[0].freeze_error);
        table.toggle_freeze(0, Some(&handle)).unwrap();
        assert!(!table.entries[0].frozen);
        assert!(!table.entries[0].freeze_error);
    }

    #[test]
    fn test_clear_freezes() {
        let handle = MockHandle::new(0x1000, vec![0u8; 8]);
        let mut table = AddressTable::new();
        table.add(AddressEntry::new(0x1000, ValueType::U32, "a".into()));
        table.add(AddressEntry::new(0x1004, ValueType::U32, "b".into()));
        table.toggle_freeze(0, Some(&handle)).unwrap();
        assert!(table.has_frozen());

        table.clear_freezes();
        assert!(!table.has_frozen());
        // Rows survive, only the locks are dropped.
        assert_eq!(table.entries.len(), 2);
        assert!(table.entries[0].frozen_value.is_none());
        assert!(table.entries[0].current_value.is_none());
        assert_eq!(table.freeze_error_count(), 0);
    }

    #[test]
    fn test_has_frozen() {
        let handle = MockHandle::new(0x1000, 100u32.to_le_bytes().to_vec());
        let mut table = AddressTable::new();
        table.add(AddressEntry::new(0x1000, ValueType::U32, "test".into()));
        assert!(!table.has_frozen());

        table.toggle_freeze(0, Some(&handle)).unwrap();
        assert!(table.has_frozen());
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
        assert!(!restored.entries[0].freeze_error);
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
