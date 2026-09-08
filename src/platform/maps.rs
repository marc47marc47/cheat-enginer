//! `/proc/<pid>/maps` parsing.
//!
//! Split out of `linux.rs` so it compiles - and is tested - on every platform.
//! It is pure string handling, and it is the part of the Android path most
//! likely to be wrong in a way that only shows up on a device.

use super::MemoryRegion;

/// Mappings that must never be read, whatever the caller asked for.
///
/// Reading a device mapping can block indefinitely or poke a driver
/// (`/dev/kgsl-3d0` is the Adreno GPU, `/dev/mali0` the Mali one, `/dev/binder`
/// is IPC), and the kernel pseudo-mappings below return `EIO` on an ordinary
/// read. None of them can hold a value worth scanning for, so excluding them
/// costs nothing and keeps the scanner from hanging the host process.
pub fn is_unsafe_to_read(path: Option<&str>) -> bool {
    match path {
        Some(p) => {
            p.starts_with("/dev/")
                || matches!(p, "[vvar]" | "[vvar_vclock]" | "[vsyscall]" | "[vectors]")
        }
        None => false,
    }
}

/// Parse `/proc/<pid>/maps`, dropping the mappings that are unsafe to read.
///
/// The pathname column is kept: on Android it is the only way to tell an
/// anonymous heap from a GPU device mapping, and the named anonymous regions
/// (`[anon:libc_malloc]`, `[anon:dalvik-main space (region space)]`) are
/// exactly where an app's own values live.
pub fn parse_maps(maps: &str) -> Vec<MemoryRegion> {
    let mut regions = Vec::new();

    for line in maps.lines() {
        // Layout: `addr perms offset dev inode pathname`. Only the first five
        // fields are space-free; the pathname is whatever remains, and it can
        // contain spaces - `[anon:dalvik-main space (region space)]` is the
        // region a scan most wants to reach, so splitting on whitespace and
        // taking field six would drop it.
        let mut rest = line;
        let mut fields = ["", "", "", "", ""];
        let mut complete = true;
        for field in &mut fields {
            let start = rest.trim_start();
            if start.is_empty() {
                complete = false;
                break;
            }
            let end = start.find(char::is_whitespace).unwrap_or(start.len());
            *field = &start[..end];
            rest = &start[end..];
        }
        if !complete {
            continue;
        }

        let Some((start, end)) = fields[0].split_once('-') else {
            continue;
        };
        let (Ok(start), Ok(end)) = (
            usize::from_str_radix(start, 16),
            usize::from_str_radix(end, 16),
        ) else {
            continue;
        };
        // `checked_sub` rather than `end - start`: a truncated line must be
        // skipped, not turned into a region of underflowed size.
        let Some(size) = end.checked_sub(start).filter(|s| *s > 0) else {
            continue;
        };

        let perms = fields[1];
        let path = Some(rest.trim()).filter(|p| !p.is_empty()).map(String::from);

        if is_unsafe_to_read(path.as_deref()) {
            continue;
        }

        regions.push(MemoryRegion {
            base_address: start,
            size,
            readable: perms.contains('r'),
            writable: perms.contains('w'),
            path,
        });
    }

    regions
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = concat!(
        "12c00000-12d00000 rw-p 00000000 00:00 0 [anon:dalvik-main space (region space)]\n",
        "7f0000000000-7f0000001000 r-xp 00000000 fd:00 1234 /system/lib64/libc.so\n",
        "7f0000001000-7f0000002000 rw-p 00001000 fd:00 1234 /system/lib64/libc.so\n",
        "7fa000000000-7fa000001000 rw-s 00000000 00:0c 5678 /dev/kgsl-3d0\n",
        "7ffd00000000-7ffd00001000 r--p 00000000 00:00 0 [vvar]\n",
        "7ffd00002000-7ffd00003000 ---p 00000000 00:00 0 \n",
        "7ffd00004000-7ffd00005000 rw-p 00000000 00:00 0 \n",
    );

    #[test]
    fn test_device_and_kernel_mappings_are_dropped() {
        let regions = parse_maps(SAMPLE);
        assert!(
            !regions
                .iter()
                .any(|r| r.path.as_deref() == Some("/dev/kgsl-3d0")),
            "device mappings must never reach the scanner"
        );
        assert!(!regions.iter().any(|r| r.path.as_deref() == Some("[vvar]")));
    }

    #[test]
    fn test_path_with_spaces_is_kept_whole() {
        let regions = parse_maps(SAMPLE);
        let dalvik = regions
            .iter()
            .find(|r| r.path.as_deref() == Some("[anon:dalvik-main space (region space)]"))
            .expect("the ART heap is where an app's own values live");
        assert_eq!(dalvik.base_address, 0x12c0_0000);
        assert_eq!(dalvik.size, 0x10_0000);
        assert!(dalvik.readable && dalvik.writable);
    }

    #[test]
    fn test_writable_file_mappings_survive() {
        // A .so's rw-p data segment holds globals and statics - a legitimate
        // scan target, so having a path must not exclude it.
        let regions = parse_maps(SAMPLE);
        let data_seg = regions
            .iter()
            .find(|r| r.path.as_deref() == Some("/system/lib64/libc.so") && r.writable)
            .expect("a .so data segment is scannable");
        assert_eq!(data_seg.base_address, 0x7f00_0000_1000);
    }

    #[test]
    fn test_permissions_and_pathless_regions() {
        let regions = parse_maps(SAMPLE);
        let anon: Vec<_> = regions.iter().filter(|r| r.path.is_none()).collect();
        assert_eq!(anon.len(), 2, "the ---p guard page and the rw-p anon region");
        let guard = anon.iter().find(|r| !r.readable).unwrap();
        assert!(!guard.writable);
    }

    #[test]
    fn test_malformed_lines_are_skipped() {
        // Too few fields, unparsable addresses, and a zero-length range.
        let maps = concat!(
            "garbage\n",
            "\n",
            "zzzzzz-zzzzzz rw-p 00000000 00:00 0 \n",
            "1000-1000 rw-p 00000000 00:00 0 \n",
            "2000-1000 rw-p 00000000 00:00 0 \n",
        );
        assert!(parse_maps(maps).is_empty());
    }
}
