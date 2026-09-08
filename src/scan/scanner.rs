use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use rayon::prelude::*;

use crate::error::Result;
use crate::platform::ProcessHandle;

use super::filter;
use super::value_type::{ScanType, ScanValue, ValueType};

#[derive(Debug, Clone)]
pub struct ScanResult {
    pub address: usize,
    pub value: ScanValue,
}

pub struct ScanProgress {
    /// Set by the scanner once it knows how many regions survived filtering.
    /// The caller's initial estimate counts every mapping, including the
    /// read-only ones that are never scanned.
    pub total_regions: AtomicUsize,
    pub scanned_regions: AtomicUsize,
    pub found_count: AtomicUsize,
    pub cancelled: AtomicBool,
    /// Set when a limit in [`ScanLimits`] cut the pass short, so the UI can say
    /// the list is capped rather than pretending it is complete.
    pub truncated: AtomicBool,
}

impl ScanProgress {
    pub fn new(total: usize) -> Self {
        Self {
            total_regions: AtomicUsize::new(total),
            scanned_regions: AtomicUsize::new(0),
            found_count: AtomicUsize::new(0),
            cancelled: AtomicBool::new(false),
            truncated: AtomicBool::new(false),
        }
    }

    pub fn percentage(&self) -> f64 {
        let total = self.total_regions.load(Ordering::Relaxed);
        if total == 0 {
            return 100.0;
        }
        (self.scanned_regions.load(Ordering::Relaxed) as f64 / total as f64) * 100.0
    }
}

/// Ceilings that keep a scan from taking the process down with it.
///
/// A desktop scan can afford to be greedy; an Android overlay lives inside the
/// app it is scanning, so an unbounded snapshot gets the host OOM-killed and
/// the user never learns why. The defaults are generous enough to be invisible
/// on a desktop target.
#[derive(Debug, Clone, Copy)]
pub struct ScanLimits {
    /// Stop collecting once this many addresses match.
    pub max_results: usize,
    /// Total bytes an Unknown-Initial snapshot may retain.
    pub max_snapshot_bytes: usize,
    /// Skip any single mapping larger than this. Android hands out
    /// multi-hundred-megabyte ART regions not worth one allocation.
    pub max_region_bytes: usize,
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            max_results: 2_000_000,
            max_snapshot_bytes: 256 * 1024 * 1024,
            max_region_bytes: 512 * 1024 * 1024,
        }
    }
}

pub struct Scanner {
    results: Vec<ScanResult>,
    snapshot: Vec<(usize, Vec<u8>)>,
    value_type: ValueType,
    has_scanned: bool,
    limits: ScanLimits,
    /// Set by an Unknown-Initial scan and cleared once a filtering pass has
    /// turned the snapshot into a candidate list. Every `first_scan` leaves a
    /// snapshot behind, so the snapshot alone cannot tell the two apart.
    snapshot_pending: bool,
}

impl Scanner {
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
            snapshot: Vec::new(),
            value_type: ValueType::U32,
            has_scanned: false,
            limits: ScanLimits::default(),
            snapshot_pending: false,
        }
    }

    pub fn set_limits(&mut self, limits: ScanLimits) {
        self.limits = limits;
    }

    pub fn limits(&self) -> ScanLimits {
        self.limits
    }

    pub fn set_value_type(&mut self, vt: ValueType) {
        self.value_type = vt;
    }

    pub fn value_type(&self) -> ValueType {
        self.value_type
    }

    pub fn has_scanned(&self) -> bool {
        self.has_scanned
    }

    pub fn results(&self) -> &[ScanResult] {
        &self.results
    }

    pub fn result_count(&self) -> usize {
        self.results.len()
    }

    /// Bytes currently pinned by the Unknown-Initial snapshot. Worth showing on
    /// a phone, where this is the number that decides whether the host app
    /// survives the next scan.
    pub fn snapshot_bytes(&self) -> usize {
        self.snapshot.iter().map(|(_, d)| d.len()).sum()
    }

    /// True when an Unknown-Initial scan has captured memory but no candidate
    /// list exists yet. In that state `result_count()` is zero and yet the next
    /// scan still has everything it needs (see `next_scan`).
    pub fn snapshot_pending(&self) -> bool {
        self.snapshot_pending
    }

    pub fn reset(&mut self) {
        self.results.clear();
        self.snapshot.clear();
        self.has_scanned = false;
        self.snapshot_pending = false;
    }

    pub fn first_scan(
        &mut self,
        handle: &dyn ProcessHandle,
        scan_type: ScanType,
        target: Option<&ScanValue>,
        progress: Option<Arc<ScanProgress>>,
    ) -> Result<usize> {
        let regions = handle.memory_regions()?;
        // Only writable regions can hold values worth scanning for - a
        // match in .text or .rdata can never be modified.
        let scan_regions: Vec<_> = regions
            .into_iter()
            .filter(|r| r.readable && r.writable && r.size <= self.limits.max_region_bytes)
            .collect();

        if let Some(ref p) = progress {
            p.scanned_regions.store(0, Ordering::Relaxed);
            p.found_count.store(0, Ordering::Relaxed);
            p.total_regions
                .store(scan_regions.len(), Ordering::Relaxed);
        }

        let vt = self.value_type;
        let step = vt.size();

        // Relative modes have nothing to compare against on a first pass, so
        // they capture memory exactly like an Unknown-Initial scan does and let
        // the next pass do the real comparison. Comparing the freshly read
        // bytes against themselves would make Unchanged match everything and
        // Increased/Decreased match nothing.
        if scan_type == ScanType::UnknownInitial || scan_type.needs_previous() {
            let mut snapshot = Vec::new();
            let mut snapshot_bytes = 0usize;
            for region in &scan_regions {
                if progress.as_ref().is_some_and(|p| p.cancelled.load(Ordering::Relaxed)) {
                    break;
                }
                // Stop before the cap rather than half-capturing a region: a
                // partially snapshotted mapping would compare garbage on the
                // next pass. Regions past the cap are simply not candidates.
                if snapshot_bytes + region.size > self.limits.max_snapshot_bytes {
                    if let Some(ref p) = progress {
                        p.truncated.store(true, Ordering::Relaxed);
                    }
                    break;
                }
                if let Ok(data) = handle.read_memory(region.base_address, region.size) {
                    snapshot_bytes += data.len();
                    snapshot.push((region.base_address, data));
                }
                if let Some(ref p) = progress {
                    p.scanned_regions.fetch_add(1, Ordering::Relaxed);
                }
            }
            self.snapshot = snapshot;
            self.results.clear();
            self.has_scanned = true;
            self.snapshot_pending = true;
            return Ok(0);
        }

        let region_data: Vec<(usize, Vec<u8>)> = scan_regions
            .iter()
            .filter_map(|region| {
                handle
                    .read_memory(region.base_address, region.size)
                    .ok()
                    .map(|data| (region.base_address, data))
            })
            .collect();

        // A shared counter rather than trimming afterwards: on a wide-open
        // scan the difference is whether the result vector stays bounded or
        // eats the heap before anyone gets to trim it.
        let cap = self.limits.max_results;
        let found = AtomicUsize::new(0);

        let mut results: Vec<ScanResult> = region_data
            .par_iter()
            .flat_map(|(base, data)| {
                let mut local_results = Vec::new();
                if data.len() < step {
                    return local_results;
                }
                for offset in (0..=data.len() - step).step_by(1) {
                    if found.load(Ordering::Relaxed) >= cap {
                        break;
                    }
                    if let Some(val) = ScanValue::from_bytes(&data[offset..], vt) {
                        // Only the target-comparing modes reach here, and those
                        // ignore `previous` - the relative ones took the
                        // snapshot path above.
                        if filter::compare(&val, &val, scan_type, target) {
                            found.fetch_add(1, Ordering::Relaxed);
                            local_results.push(ScanResult {
                                address: base + offset,
                                value: val,
                            });
                        }
                    }
                }
                local_results
            })
            .collect();

        let capped = results.len() >= cap;
        results.truncate(cap);
        if let Some(ref p) = progress {
            p.truncated.store(capped, Ordering::Relaxed);
        }

        if let Some(ref p) = progress {
            p.found_count.store(results.len(), Ordering::Relaxed);
            p.scanned_regions
                .store(scan_regions.len(), Ordering::Relaxed);
        }

        // Only keep the raw bytes when the scan found nothing - that is the
        // one case `next_scan` still needs them for, falling back to diffing
        // the snapshot. Holding every writable region after a scan that did
        // find something is what makes an in-process Android scan run out of
        // memory.
        self.snapshot = if results.is_empty() {
            region_data
        } else {
            Vec::new()
        };
        self.results = results;
        self.has_scanned = true;
        self.snapshot_pending = false;
        Ok(self.results.len())
    }

    pub fn next_scan(
        &mut self,
        handle: &dyn ProcessHandle,
        scan_type: ScanType,
        target: Option<&ScanValue>,
        progress: Option<Arc<ScanProgress>>,
    ) -> Result<usize> {
        if !self.has_scanned {
            return self.first_scan(handle, scan_type, target, progress);
        }

        let vt = self.value_type;

        if self.results.is_empty() && !self.snapshot.is_empty() {
            let step = vt.size();
            let mut new_results = Vec::new();
            let mut new_snapshot = Vec::new();

            if let Some(ref p) = progress {
                p.total_regions.store(self.snapshot.len(), Ordering::Relaxed);
                p.scanned_regions.store(0, Ordering::Relaxed);
            }

            for (base, old_data) in &self.snapshot {
                if let Ok(new_data) = handle.read_memory(*base, old_data.len()) {
                    if new_data.len() < step {
                        continue;
                    }
                    for offset in (0..=new_data.len() - step).step_by(1) {
                        if let (Some(current), Some(previous)) = (
                            ScanValue::from_bytes(&new_data[offset..], vt),
                            ScanValue::from_bytes(&old_data[offset..], vt),
                        ) {
                            if filter::compare(&current, &previous, scan_type, target) {
                                new_results.push(ScanResult {
                                    address: base + offset,
                                    value: current,
                                });
                            }
                        }
                    }
                    new_snapshot.push((*base, new_data));
                }
                if let Some(ref p) = progress {
                    p.scanned_regions.fetch_add(1, Ordering::Relaxed);
                }
            }

            if let Some(ref p) = progress {
                p.found_count.store(new_results.len(), Ordering::Relaxed);
            }

            self.snapshot = new_snapshot;
            self.results = new_results;
            self.snapshot_pending = false;
            return Ok(self.results.len());
        }

        let old_results = std::mem::take(&mut self.results);
        let mut new_results = Vec::new();

        // Narrowing an existing candidate list walks addresses, not regions;
        // report progress in those units so the bar still moves.
        if let Some(ref p) = progress {
            p.total_regions.store(old_results.len(), Ordering::Relaxed);
            p.scanned_regions.store(0, Ordering::Relaxed);
        }

        for (i, result) in old_results.iter().enumerate() {
            if let Some(ref p) = progress {
                if i % 1024 == 0 {
                    p.scanned_regions.store(i, Ordering::Relaxed);
                }
            }
            if let Ok(data) = handle.read_memory(result.address, vt.size()) {
                if let Some(current) = ScanValue::from_bytes(&data, vt) {
                    if filter::compare(&current, &result.value, scan_type, target) {
                        new_results.push(ScanResult {
                            address: result.address,
                            value: current,
                        });
                    }
                }
            }
        }

        if let Some(ref p) = progress {
            p.found_count.store(new_results.len(), Ordering::Relaxed);
        }

        self.results = new_results;
        Ok(self.results.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::MemoryRegion;
    use crate::scan::value_type::ValueType;
    use std::sync::Mutex;

    const BASE: usize = 0x1000;

    struct FakeHandle {
        memory: Mutex<Vec<u8>>,
        /// Reported region size, which may be a lie much larger than `memory`
        /// so the region-size cap can be exercised without allocating it.
        claimed_size: usize,
    }

    impl FakeHandle {
        fn new(memory: Vec<u8>) -> Self {
            let claimed_size = memory.len();
            Self {
                memory: Mutex::new(memory),
                claimed_size,
            }
        }

        fn claiming(mut self, size: usize) -> Self {
            self.claimed_size = size;
            self
        }

        fn set_u32(&self, offset: usize, value: u32) {
            let mut mem = self.memory.lock().unwrap();
            mem[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
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
            // Deliberately short-read rather than failing, the way the real
            // /proc handle does at the end of a mapping.
            let end = (offset + size).min(mem.len());
            Ok(mem[offset..end].to_vec())
        }

        fn write_memory(&self, address: usize, data: &[u8]) -> Result<()> {
            let mut mem = self.memory.lock().unwrap();
            let offset = address - BASE;
            mem[offset..offset + data.len()].copy_from_slice(data);
            Ok(())
        }

        fn memory_regions(&self) -> Result<Vec<MemoryRegion>> {
            Ok(vec![MemoryRegion {
                base_address: BASE,
                size: self.claimed_size,
                readable: true,
                writable: true,
                path: None,
            }])
        }
    }

    /// Four u32 values: 10, 20, 30, 40.
    fn sample_handle() -> FakeHandle {
        let mut mem = Vec::new();
        for v in [10u32, 20, 30, 40] {
            mem.extend_from_slice(&v.to_le_bytes());
        }
        FakeHandle::new(mem)
    }

    #[test]
    fn test_relative_first_scan_captures_instead_of_matching_everything() {
        let handle = sample_handle();
        let mut scanner = Scanner::new();

        // Comparing freshly read bytes against themselves used to make
        // Unchanged match every address in the process and Increased match
        // none. A first pass has no previous value, so it snapshots instead.
        for scan_type in [
            ScanType::Increased,
            ScanType::Decreased,
            ScanType::Changed,
            ScanType::Unchanged,
        ] {
            scanner.reset();
            let found = scanner
                .first_scan(&handle, scan_type, None, None)
                .expect("snapshot pass succeeds");
            assert_eq!(found, 0, "{scan_type:?} cannot match on a first pass");
            assert!(scanner.snapshot_pending(), "{scan_type:?} must leave a snapshot");
            assert!(scanner.has_scanned());
        }
    }

    #[test]
    fn test_next_scan_after_relative_first_scan_finds_the_change() {
        let handle = sample_handle();
        let mut scanner = Scanner::new();

        scanner.first_scan(&handle, ScanType::Increased, None, None).unwrap();
        handle.set_u32(8, 99); // third value: 30 -> 99

        scanner
            .next_scan(&handle, ScanType::Increased, None, None)
            .unwrap();

        let addresses: Vec<_> = scanner.results().iter().map(|r| r.address).collect();
        assert!(
            addresses.contains(&(BASE + 8)),
            "the value that grew must survive, got {addresses:x?}"
        );
        // Scanning is byte-granular, so the unaligned reads straddling the
        // changed byte legitimately match too - but nothing below it can.
        assert!(
            addresses.iter().all(|a| *a > BASE + 4),
            "untouched values must be dropped, got {addresses:x?}"
        );
        assert!(!scanner.snapshot_pending());
    }

    #[test]
    fn test_successful_exact_scan_does_not_pin_the_snapshot() {
        let handle = sample_handle();
        let mut scanner = Scanner::new();
        let target = ScanValue::U32(20);

        let found = scanner
            .first_scan(&handle, ScanType::ExactValue, Some(&target), None)
            .unwrap();

        assert_eq!(found, 1);
        // Retaining every writable region after a scan that found something is
        // what gets an in-process Android scan OOM-killed.
        assert_eq!(scanner.snapshot_bytes(), 0);
    }

    #[test]
    fn test_empty_exact_scan_keeps_the_snapshot_for_the_next_pass() {
        let handle = sample_handle();
        let mut scanner = Scanner::new();
        let target = ScanValue::U32(7777);

        assert_eq!(
            scanner
                .first_scan(&handle, ScanType::ExactValue, Some(&target), None)
                .unwrap(),
            0
        );
        assert!(scanner.snapshot_bytes() > 0, "a dead end must stay narrowable");
    }

    #[test]
    fn test_result_cap_is_enforced_and_reported() {
        // 64 identical u32s, so an exact scan would otherwise match at every
        // 4-byte-aligned offset and then some.
        let handle = FakeHandle::new([5u8; 256].to_vec());
        let mut scanner = Scanner::new();
        scanner.set_limits(ScanLimits {
            max_results: 4,
            ..ScanLimits::default()
        });
        let progress = Arc::new(ScanProgress::new(1));
        let target = ScanValue::U32(u32::from_le_bytes([5, 5, 5, 5]));

        let found = scanner
            .first_scan(
                &handle,
                ScanType::ExactValue,
                Some(&target),
                Some(Arc::clone(&progress)),
            )
            .unwrap();

        assert_eq!(found, 4);
        assert!(progress.truncated.load(Ordering::Relaxed));
    }

    #[test]
    fn test_oversized_regions_are_skipped() {
        // Android hands out multi-hundred-megabyte ART mappings; one of them is
        // a single allocation the host app cannot afford.
        let handle = sample_handle().claiming(usize::MAX / 2);
        let mut scanner = Scanner::new();
        scanner.set_limits(ScanLimits {
            max_region_bytes: 1024,
            ..ScanLimits::default()
        });
        let target = ScanValue::U32(20);

        let found = scanner
            .first_scan(&handle, ScanType::ExactValue, Some(&target), None)
            .unwrap();

        assert_eq!(found, 0, "the region should never have been read");
    }

    #[test]
    fn test_value_type_is_preserved_across_reset() {
        let mut scanner = Scanner::new();
        scanner.set_value_type(ValueType::F32);
        scanner.reset();
        assert_eq!(scanner.value_type(), ValueType::F32);
        assert!(!scanner.has_scanned());
    }
}
