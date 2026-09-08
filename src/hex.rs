//! The hex dump as a data model, separate from any way of drawing it.
//!
//! The TUI used to read process memory from inside its render function, sized
//! by whatever height ratatui had just computed - so scrolling the terminal
//! issued syscalls, and the amount read depended on the window. Both front-ends
//! now ask this model to `refresh` on their own clock and then render whatever
//! it holds.

use crate::error::Result;
use crate::platform::ProcessHandle;

/// A window onto process memory, with the previous read kept so a renderer can
/// highlight what changed.
pub struct HexModel {
    address: usize,
    bytes_per_row: usize,
    rows: usize,
    previous: Vec<u8>,
    current: Vec<u8>,
}

impl HexModel {
    pub fn new() -> Self {
        Self {
            address: 0,
            bytes_per_row: 16,
            rows: 16,
            previous: Vec::new(),
            current: Vec::new(),
        }
    }

    pub fn address(&self) -> usize {
        self.address
    }

    pub fn set_address(&mut self, address: usize) {
        if address != self.address {
            // Byte-change highlighting is meaningless across a jump.
            self.previous.clear();
            self.current.clear();
        }
        self.address = address;
    }

    pub fn bytes_per_row(&self) -> usize {
        self.bytes_per_row
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    /// How many rows the renderer can actually show. Recorded rather than acted
    /// on, so that layout still decides the size but never triggers a read.
    pub fn set_rows(&mut self, rows: usize) {
        self.rows = rows.max(1);
    }

    pub fn scroll_up(&mut self) {
        self.set_address(self.address.saturating_sub(self.bytes_per_row));
    }

    pub fn scroll_down(&mut self) {
        self.set_address(self.address.saturating_add(self.bytes_per_row));
    }

    pub fn page_up(&mut self) {
        self.set_address(
            self.address
                .saturating_sub(self.bytes_per_row * self.rows),
        );
    }

    pub fn page_down(&mut self) {
        self.set_address(
            self.address
                .saturating_add(self.bytes_per_row * self.rows),
        );
    }

    /// Re-read the window. Short reads are kept - a page may end mid-window,
    /// and showing the readable prefix beats showing nothing.
    pub fn refresh(&mut self, handle: &dyn ProcessHandle) -> Result<()> {
        let len = self.bytes_per_row * self.rows;
        let data = handle.read_memory(self.address, len)?;
        self.previous = std::mem::replace(&mut self.current, data);
        Ok(())
    }

    pub fn bytes(&self) -> &[u8] {
        &self.current
    }

    pub fn byte_at(&self, index: usize) -> Option<u8> {
        self.current.get(index).copied()
    }

    /// True when this byte differs from the previous read. False on the first
    /// read, when there is nothing to compare against.
    pub fn changed_at(&self, index: usize) -> bool {
        match (self.previous.get(index), self.current.get(index)) {
            (Some(old), Some(new)) => old != new,
            _ => false,
        }
    }
}

impl Default for HexModel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::MemoryRegion;
    use std::sync::Mutex;

    const BASE: usize = 0x1000;

    struct FakeHandle {
        memory: Mutex<Vec<u8>>,
    }

    impl ProcessHandle for FakeHandle {
        fn read_memory(&self, address: usize, size: usize) -> Result<Vec<u8>> {
            let mem = self.memory.lock().unwrap();
            let offset = address
                .checked_sub(BASE)
                .ok_or_else(|| anyhow::anyhow!("out of range"))?;
            if offset >= mem.len() {
                return Err(anyhow::anyhow!("out of range"));
            }
            Ok(mem[offset..(offset + size).min(mem.len())].to_vec())
        }

        fn write_memory(&self, _address: usize, _data: &[u8]) -> Result<()> {
            Ok(())
        }

        fn memory_regions(&self) -> Result<Vec<MemoryRegion>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn test_refresh_tracks_changed_bytes() {
        let handle = FakeHandle {
            memory: Mutex::new(vec![0u8; 64]),
        };
        let mut model = HexModel::new();
        model.set_address(BASE);
        model.set_rows(2);

        model.refresh(&handle).unwrap();
        assert!(
            !model.changed_at(3),
            "nothing has changed on the very first read"
        );

        handle.memory.lock().unwrap()[3] = 0xAB;
        model.refresh(&handle).unwrap();

        assert_eq!(model.byte_at(3), Some(0xAB));
        assert!(model.changed_at(3));
        assert!(!model.changed_at(4));
    }

    #[test]
    fn test_jumping_clears_the_comparison() {
        let handle = FakeHandle {
            memory: Mutex::new((0..64u8).collect()),
        };
        let mut model = HexModel::new();
        model.set_address(BASE);
        model.set_rows(1);
        model.refresh(&handle).unwrap();

        model.set_address(BASE + 16);
        model.refresh(&handle).unwrap();

        // Every byte differs from the old window, but that is a different
        // address range - reporting it as "changed" would be a lie.
        assert!((0..16).all(|i| !model.changed_at(i)));
    }

    #[test]
    fn test_short_read_is_kept() {
        let handle = FakeHandle {
            memory: Mutex::new(vec![7u8; 20]),
        };
        let mut model = HexModel::new();
        model.set_address(BASE);
        model.set_rows(4); // asks for 64 bytes, only 20 exist

        model.refresh(&handle).unwrap();
        assert_eq!(model.bytes().len(), 20);
        assert_eq!(model.byte_at(19), Some(7));
        assert_eq!(model.byte_at(20), None);
    }

    #[test]
    fn test_scrolling_moves_by_a_row() {
        let mut model = HexModel::new();
        model.set_address(BASE);
        model.set_rows(4);

        model.scroll_down();
        assert_eq!(model.address(), BASE + 16);
        model.page_down();
        assert_eq!(model.address(), BASE + 16 + 64);
        model.page_up();
        assert_eq!(model.address(), BASE + 16);
        model.scroll_up();
        assert_eq!(model.address(), BASE);

        // Never wraps below zero.
        model.set_address(4);
        model.page_up();
        assert_eq!(model.address(), 0);
    }
}
