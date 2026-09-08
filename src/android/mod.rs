//! JNI bridge for the embeddable Android overlay.
//!
//! A deliberately thin shim: every call resolves an opaque [`Session`] pointer,
//! does one marshalling step, and returns. All logic, threading and state live
//! in [`crate::session`], so the Kotlin side owns nothing that has to survive
//! an Activity being destroyed.
//!
//! Two rules hold everywhere in this file:
//!
//! * **Nothing may unwind into the JVM.** A Rust panic crossing an
//!   `extern "system"` boundary aborts the whole host app, so every exported
//!   function runs inside [`guard`]. That also means no profile in the Android
//!   build may set `panic = "abort"`, which would turn the guard into a no-op.
//! * **No JNI is called from a Rust-spawned thread.** The scan and freeze
//!   threads touch only Rust data and atomics; Kotlin polls. That removes the
//!   whole `AttachCurrentThread` failure class.

use std::mem::ManuallyDrop;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use jni::JNIEnv;
use jni::objects::{JClass, JLongArray, JObject, JString};
use jni::sys::{jboolean, jint, jlong};

use crate::scan::scanner::ScanLimits;
use crate::scan::value_type::{ScanType, ScanValue, ValueType};
use crate::session::{ScanRequest, Session};

/// Bytes per packed result record. Kotlin decodes these with a little-endian
/// `ByteBuffer`; a page of a few hundred costs one array instead of one JNI
/// round trip per row.
const RECORD_SIZE: usize = 24;

/// Run a body that must never unwind into the JVM.
fn guard<R>(fallback: R, body: impl FnOnce() -> R) -> R {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or(fallback)
}

/// Borrow the session behind an opaque handle without taking ownership of it.
///
/// # Safety
/// `handle` must be a pointer produced by `nativeInit` and not yet destroyed.
unsafe fn with_session<R>(handle: jlong, fallback: R, body: impl FnOnce(&Arc<Session>) -> R) -> R {
    if handle == 0 {
        return fallback;
    }
    let session = ManuallyDrop::new(unsafe { Arc::from_raw(handle as *const Session) });
    body(&session)
}

fn jstring_to_string(env: &mut JNIEnv, s: &JString) -> Option<String> {
    if s.is_null() {
        return None;
    }
    env.get_string(s).ok().map(|v| v.into())
}

fn string_out(env: &mut JNIEnv, value: &str) -> jni::sys::jstring {
    match env.new_string(value) {
        Ok(s) => s.into_raw(),
        Err(_) => JObject::null().into_raw(),
    }
}

/// Little-endian bytes of a value, zero-extended to 8 so every record is the
/// same width. Floats travel as their bit pattern, which is what
/// `Float.fromBits` on the Kotlin side expects.
fn value_bits(value: &ScanValue) -> [u8; 8] {
    let mut out = [0u8; 8];
    let bytes = value.to_bytes();
    let n = bytes.len().min(8);
    out[..n].copy_from_slice(&bytes[..n]);
    out
}

fn push_record(buf: &mut Vec<u8>, address: usize, value: &ScanValue, vt: ValueType, flags: u32) {
    buf.extend_from_slice(&(address as u64).to_le_bytes());
    buf.extend_from_slice(&value_bits(value));
    buf.extend_from_slice(&(vt.index() as u32).to_le_bytes());
    buf.extend_from_slice(&flags.to_le_bytes());
}

fn byte_array_out(env: &mut JNIEnv, bytes: &[u8]) -> jni::sys::jbyteArray {
    match env.byte_array_from_slice(bytes) {
        Ok(a) => a.into_raw(),
        Err(_) => JObject::null().into_raw(),
    }
}

// ---------------------------------------------------------------------------
// lifecycle
// ---------------------------------------------------------------------------

/// Attach to the calling process and start the freeze thread.
///
/// Returns 0 on failure; the reason is then available from `nativeLastError`
/// with a 0 handle, so the caller can still show it.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeInit(
    _env: JNIEnv,
    _class: JClass,
) -> jlong {
    guard(0, || match Session::attach_self() {
        Ok(session) => {
            // Phone-sized ceilings, not the desktop defaults. This engine runs
            // inside the app it is scanning: an Unknown-Initial pass with the
            // desktop caps would retain 256 MB of snapshot and get the host
            // OOM-killed, with nothing in logcat to explain why.
            session.set_limits(ScanLimits {
                max_results: 200_000,
                max_snapshot_bytes: 32 * 1024 * 1024,
                max_region_bytes: 64 * 1024 * 1024,
            });
            Arc::into_raw(session) as jlong
        }
        Err(_) => 0,
    })
}

/// Stop the worker threads, then drop the session.
///
/// The shutdown must complete before the memory goes away: a freeze thread
/// still dereferencing a freed session takes the host app down with it.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeDestroy(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    guard((), || {
        if handle == 0 {
            return;
        }
        let session = unsafe { Arc::from_raw(handle as *const Session) };
        session.shutdown();
        drop(session);
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeVersionLine(
    mut env: JNIEnv,
    _class: JClass,
) -> jni::sys::jstring {
    guard(JObject::null().into_raw(), || {
        string_out(&mut env, crate::VERSION_LINE)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeSelfPid(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    guard(0, || std::process::id() as jint)
}

/// Take and clear the last error, or `null` when there is none.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeLastError(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jni::sys::jstring {
    guard(JObject::null().into_raw(), || unsafe {
        with_session(handle, JObject::null().into_raw(), |session| {
            match session.take_last_error() {
                Some(msg) => string_out(&mut env, &msg),
                None => JObject::null().into_raw(),
            }
        })
    })
}

// ---------------------------------------------------------------------------
// scanning
// ---------------------------------------------------------------------------

/// Start a scan on a Rust thread. Returns true if it was accepted.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeStartScan(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    value_type: jint,
    scan_type: jint,
    target: JString,
    restart: jboolean,
) -> jboolean {
    guard(0, || {
        let text = jstring_to_string(&mut env, &target);
        unsafe {
            with_session(handle, 0, |session| {
                let Some(vt) = ValueType::from_index(value_type.max(0) as usize) else {
                    return 0;
                };
                let Some(st) = ScanType::from_index(scan_type.max(0) as usize) else {
                    return 0;
                };
                let mut request = ScanRequest::new(vt, st);
                request.target_text = text;
                request.restart = restart != 0;

                match session.start_scan(request) {
                    Ok(()) => 1,
                    Err(e) => {
                        // Stored rather than thrown: the caller is a button
                        // handler that wants a message, not an exception.
                        session.record_error(e.to_string());
                        0
                    }
                }
            })
        }
    })
}

/// `[state, scannedRegions, totalRegions, found, truncated]`.
///
/// Every value is read from an atomic, so this is safe to poll on a UI timer
/// even while a scan holds the scanner lock.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeScanStatus(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jni::sys::jlongArray {
    guard(JObject::null().into_raw(), || unsafe {
        with_session(handle, JObject::null().into_raw(), |session| {
            let status = session.status();
            let values = [
                status.state as u8 as jlong,
                status.scanned_regions as jlong,
                status.total_regions as jlong,
                status.found as jlong,
                jlong::from(status.truncated),
            ];
            let Ok(array) = env.new_long_array(values.len() as jni::sys::jsize) else {
                return JObject::null().into_raw();
            };
            let array: JLongArray = array;
            if env.set_long_array_region(&array, 0, &values).is_err() {
                return JObject::null().into_raw();
            }
            array.into_raw()
        })
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeCancelScan(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    guard((), || unsafe {
        with_session(handle, (), |session| session.cancel_scan())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeResetScan(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    guard((), || unsafe {
        with_session(handle, (), |session| session.reset_scan())
    })
}

/// Result count, or -1 while a scan owns the scanner.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeResultCount(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    guard(-1, || unsafe {
        with_session(handle, -1, |session| {
            session.result_count().map(|c| c as jint).unwrap_or(-1)
        })
    })
}

/// A page of results as packed [`RECORD_SIZE`]-byte little-endian records:
/// `i64 address`, `8 bytes value`, `i32 valueTypeIndex`, `i32 flags`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeResultsPage(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    offset: jint,
    count: jint,
) -> jni::sys::jbyteArray {
    guard(JObject::null().into_raw(), || unsafe {
        with_session(handle, JObject::null().into_raw(), |session| {
            let results = session.results_page(offset.max(0) as usize, count.max(0) as usize);
            let vt = session.value_type();
            let mut buf = Vec::with_capacity(results.len() * RECORD_SIZE);
            for result in &results {
                push_record(&mut buf, result.address, &result.value, vt, 0);
            }
            byte_array_out(&mut env, &buf)
        })
    })
}

// ---------------------------------------------------------------------------
// direct memory access
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeReadBytes(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    address: jlong,
    len: jint,
) -> jni::sys::jbyteArray {
    guard(JObject::null().into_raw(), || unsafe {
        with_session(handle, JObject::null().into_raw(), |session| {
            match session.read_bytes(address as usize, len.max(0) as usize) {
                Ok(bytes) => byte_array_out(&mut env, &bytes),
                Err(e) => {
                    session.record_error(e.to_string());
                    JObject::null().into_raw()
                }
            }
        })
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeWriteValue(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    address: jlong,
    value_type: jint,
    text: JString,
) -> jboolean {
    guard(0, || {
        let text = jstring_to_string(&mut env, &text).unwrap_or_default();
        unsafe {
            with_session(handle, 0, |session| {
                let Some(vt) = ValueType::from_index(value_type.max(0) as usize) else {
                    return 0;
                };
                match session.write_value_text(address as usize, vt, &text) {
                    Ok(()) => 1,
                    Err(e) => {
                        session.record_error(e.to_string());
                        0
                    }
                }
            })
        }
    })
}

// ---------------------------------------------------------------------------
// address table
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeTableAdd(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    address: jlong,
    value_type: jint,
    description: JString,
) {
    guard((), || {
        let desc = jstring_to_string(&mut env, &description).unwrap_or_default();
        unsafe {
            with_session(handle, (), |session| {
                if let Some(vt) = ValueType::from_index(value_type.max(0) as usize) {
                    session.table_add(address as usize, vt, desc);
                }
            })
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeTableRemove(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    index: jint,
) {
    guard((), || unsafe {
        with_session(handle, (), |session| {
            session.table_remove(index.max(0) as usize)
        })
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeTableToggleFreeze(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    index: jint,
) -> jboolean {
    guard(0, || unsafe {
        with_session(handle, 0, |session| {
            match session.table_toggle_freeze(index.max(0) as usize) {
                Ok(()) => 1,
                Err(e) => {
                    session.record_error(e.to_string());
                    0
                }
            }
        })
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeTableSetValue(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    index: jint,
    text: JString,
) -> jboolean {
    guard(0, || {
        let text = jstring_to_string(&mut env, &text).unwrap_or_default();
        unsafe {
            with_session(handle, 0, |session| {
                match session.table_set_value_text(index.max(0) as usize, &text) {
                    Ok(()) => 1,
                    Err(e) => {
                        session.record_error(e.to_string());
                        0
                    }
                }
            })
        }
    })
}

/// The whole table as packed records, values refreshed from memory.
///
/// Flags: bit 0 frozen, bit 1 freeze error.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeTablePage(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jni::sys::jbyteArray {
    guard(JObject::null().into_raw(), || unsafe {
        with_session(handle, JObject::null().into_raw(), |session| {
            let entries = session.table_entries();
            let mut buf = Vec::with_capacity(entries.len() * RECORD_SIZE);
            for entry in &entries {
                let flags = u32::from(entry.frozen) | (u32::from(entry.freeze_error) << 1);
                let value = entry
                    .current_value
                    .clone()
                    .unwrap_or(ScanValue::U64(0));
                push_record(&mut buf, entry.address, &value, entry.value_type, flags);
            }
            byte_array_out(&mut env, &buf)
        })
    })
}

/// Descriptions, in table order. Kept out of the packed records so the binary
/// layout stays fixed-width.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeTableDescriptions(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jni::sys::jobjectArray {
    guard(JObject::null().into_raw(), || unsafe {
        with_session(handle, JObject::null().into_raw(), |session| {
            let entries = session.table_entries();
            let Ok(class) = env.find_class("java/lang/String") else {
                return JObject::null().into_raw();
            };
            let Ok(array) = env.new_object_array(
                entries.len() as jni::sys::jsize,
                &class,
                JObject::null(),
            ) else {
                return JObject::null().into_raw();
            };
            for (i, entry) in entries.iter().enumerate() {
                if let Ok(s) = env.new_string(&entry.description) {
                    let _ = env.set_object_array_element(&array, i as jni::sys::jsize, s);
                }
            }
            array.into_raw()
        })
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeTableSave(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    path: JString,
) -> jboolean {
    guard(0, || {
        let path = jstring_to_string(&mut env, &path).unwrap_or_default();
        unsafe {
            with_session(handle, 0, |session| match session.table_save(&path) {
                Ok(()) => 1,
                Err(e) => {
                    session.record_error(e.to_string());
                    0
                }
            })
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeTableLoad(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    path: JString,
) -> jboolean {
    guard(0, || {
        let path = jstring_to_string(&mut env, &path).unwrap_or_default();
        unsafe {
            with_session(handle, 0, |session| match session.table_load(&path) {
                Ok(()) => 1,
                Err(e) => {
                    session.record_error(e.to_string());
                    0
                }
            })
        }
    })
}

// ---------------------------------------------------------------------------
// labels, so the Kotlin spinners cannot drift out of sync with the engine
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeValueTypeLabels(
    mut env: JNIEnv,
    _class: JClass,
) -> jni::sys::jobjectArray {
    guard(JObject::null().into_raw(), || {
        let labels: Vec<&str> = ValueType::ALL.iter().map(|v| v.label()).collect();
        string_array_out(&mut env, &labels)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeScanTypeLabels(
    mut env: JNIEnv,
    _class: JClass,
) -> jni::sys::jobjectArray {
    guard(JObject::null().into_raw(), || {
        let labels: Vec<&str> = ScanType::ALL.iter().map(|s| s.label()).collect();
        string_array_out(&mut env, &labels)
    })
}

/// Bit set of scan-mode indices that require a typed value, so the UI can grey
/// the field out without duplicating the rule.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_marc_ce_overlay_NativeBridge_nativeScanTypesNeedingValue(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    guard(0, || {
        ScanType::ALL
            .iter()
            .enumerate()
            .filter(|(_, s)| s.needs_target())
            .fold(0, |acc, (i, _)| acc | (1 << i))
    })
}

fn string_array_out(env: &mut JNIEnv, values: &[&str]) -> jni::sys::jobjectArray {
    let Ok(class) = env.find_class("java/lang/String") else {
        return JObject::null().into_raw();
    };
    let Ok(array) =
        env.new_object_array(values.len() as jni::sys::jsize, &class, JObject::null())
    else {
        return JObject::null().into_raw();
    };
    for (i, value) in values.iter().enumerate() {
        if let Ok(s) = env.new_string(value) {
            let _ = env.set_object_array_element(&array, i as jni::sys::jsize, s);
        }
    }
    array.into_raw()
}
