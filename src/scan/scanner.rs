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
    pub total_regions: usize,
    pub scanned_regions: AtomicUsize,
    pub found_count: AtomicUsize,
    pub cancelled: AtomicBool,
}

impl ScanProgress {
    pub fn new(total: usize) -> Self {
        Self {
            total_regions: total,
            scanned_regions: AtomicUsize::new(0),
            found_count: AtomicUsize::new(0),
            cancelled: AtomicBool::new(false),
        }
    }

    pub fn percentage(&self) -> f64 {
        if self.total_regions == 0 {
            return 100.0;
        }
        (self.scanned_regions.load(Ordering::Relaxed) as f64 / self.total_regions as f64) * 100.0
    }
}

pub struct Scanner {
    results: Vec<ScanResult>,
    snapshot: Vec<(usize, Vec<u8>)>,
    value_type: ValueType,
    has_scanned: bool,
}

impl Scanner {
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
            snapshot: Vec::new(),
            value_type: ValueType::U32,
            has_scanned: false,
        }
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

    pub fn reset(&mut self) {
        self.results.clear();
        self.snapshot.clear();
        self.has_scanned = false;
    }

    pub fn first_scan(
        &mut self,
        handle: &dyn ProcessHandle,
        scan_type: ScanType,
        target: Option<&ScanValue>,
        progress: Option<Arc<ScanProgress>>,
    ) -> Result<usize> {
        let regions = handle.memory_regions()?;
        let readable_regions: Vec<_> = regions.into_iter().filter(|r| r.readable).collect();

        if let Some(ref p) = progress {
            p.scanned_regions.store(0, Ordering::Relaxed);
            p.found_count.store(0, Ordering::Relaxed);
        }

        let vt = self.value_type;
        let step = vt.size();

        if scan_type == ScanType::UnknownInitial {
            let mut snapshot = Vec::new();
            for region in &readable_regions {
                if progress.as_ref().is_some_and(|p| p.cancelled.load(Ordering::Relaxed)) {
                    break;
                }
                if let Ok(data) = handle.read_memory(region.base_address, region.size) {
                    snapshot.push((region.base_address, data));
                }
                if let Some(ref p) = progress {
                    p.scanned_regions.fetch_add(1, Ordering::Relaxed);
                }
            }
            self.snapshot = snapshot;
            self.results.clear();
            self.has_scanned = true;
            return Ok(0);
        }

        let region_data: Vec<(usize, Vec<u8>)> = readable_regions
            .iter()
            .filter_map(|region| {
                handle
                    .read_memory(region.base_address, region.size)
                    .ok()
                    .map(|data| (region.base_address, data))
            })
            .collect();

        let results: Vec<ScanResult> = region_data
            .par_iter()
            .flat_map(|(base, data)| {
                let mut local_results = Vec::new();
                if data.len() < step {
                    return local_results;
                }
                for offset in (0..=data.len() - step).step_by(1) {
                    if let Some(val) = ScanValue::from_bytes(&data[offset..], vt) {
                        let prev = ScanValue::from_bytes(&data[offset..], vt).unwrap();
                        if filter::compare(&val, &prev, scan_type, target) {
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

        if let Some(ref p) = progress {
            p.found_count
                .store(results.len(), Ordering::Relaxed);
            p.scanned_regions
                .store(readable_regions.len(), Ordering::Relaxed);
        }

        self.snapshot = region_data;
        self.results = results;
        self.has_scanned = true;
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
            }

            if let Some(ref p) = progress {
                p.found_count.store(new_results.len(), Ordering::Relaxed);
            }

            self.snapshot = new_snapshot;
            self.results = new_results;
            return Ok(self.results.len());
        }

        let old_results = std::mem::take(&mut self.results);
        let mut new_results = Vec::new();

        for result in &old_results {
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
