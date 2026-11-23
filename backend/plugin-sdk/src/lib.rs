//! Sepia WASM plugin SDK (no_std). Build plugins with:
//!   cargo build --target wasm32-unknown-unknown --release
//!
//! ABI: the host calls `handle(ptr,len)->i64` with a JSON document
//! `{"event","payload","config"}` and reads back `(out_ptr<<32)|out_len`.
//! Plugins export `alloc(len)->i32` (bump allocator) and `memory`. The imported
//! `env.host_call(ptr,len)->i64` runs a capability and returns a packed JSON
//! `{"ok":true,"result":..}` / `{"ok":false,"error":..}`. No WASI is linked, so
//! outbound network only flows through the audited host http capability.
#![no_std]
extern crate alloc;

use alloc::alloc::alloc as heap_alloc;
use alloc::string::String;
use alloc::vec::Vec;
use core::slice;

#[link(wasm_import_module = "env")]
extern "C" {
    fn host_call(ptr: i32, len: i32) -> i64;
}

const HEAP_START: usize = 4 << 20; // 4 MiB: clear of static data + shadow stack
static mut BUMP: usize = HEAP_START;

struct BumpAlloc;
unsafe impl core::alloc::GlobalAlloc for BumpAlloc {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        let size = (layout.size() + layout.align() - 1) & !(layout.align() - 1);
        let ptr = (BUMP + layout.align() - 1) & !(layout.align() - 1);
        BUMP = ptr + size;
        ptr as *mut u8
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: core::alloc::Layout) {}
}
#[global_allocator]
static ALLOCATOR: BumpAlloc = BumpAlloc;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

#[no_mangle]
pub extern "C" fn alloc(len: i32) -> i32 {
    let size = (len.max(0) as usize + 15) & !15;
    unsafe {
        let ptr = BUMP;
        BUMP += size;
        ptr as i32
    }
}

#[no_mangle]
pub extern "C" fn sepia_grow(pages: i32) -> i32 {
    core::arch::wasm32::memory_grow(0, pages as usize) as i32
}

#[no_mangle]
pub extern "C" fn handle(ptr: i32, len: i32) -> i64 {
    let bytes = unsafe { slice::from_raw_parts(ptr as *const u8, len as usize) };
    let doc = json::parse(bytes).unwrap_or(json::Value::Null);
    let event = doc.get("event").and_then(json::Value::as_str).unwrap_or("");
    let payload = doc.get("payload").cloned().unwrap_or(json::Value::Null);
    let config = doc.get("config").cloned().unwrap_or(json::Value::Null);
    let result = unsafe { dispatch(event, &payload, &config) };
    let out = result.to_bytes();
    let layout = core::alloc::Layout::from_size_align(out.len().max(1), 1).unwrap();
    let ptr = unsafe { heap_alloc(layout) };
    unsafe {
        core::ptr::copy_nonoverlapping(out.as_ptr(), ptr, out.len());
    }
    ((ptr as i64) << 32) | out.len() as i64
}

// Implement per plugin: dispatch on `event`, return a JSON result.
extern "Rust" {
    fn dispatch(event: &str, payload: &json::Value, config: &json::Value) -> json::Value;
}

pub fn host(capability: &str, payload: json::Value) -> Result<json::Value, String> {
    let mut request = json::Value::object();
    request.insert("capability", json::Value::str(capability));
    request.insert("payload", payload);
    let bytes = request.to_bytes();
    let packed = unsafe { host_call(bytes.as_ptr() as i32, bytes.len() as i32) };
    let out_ptr = (packed >> 32) as usize;
    let out_len = (packed & 0xffff_ffff) as usize;
    let response = unsafe { slice::from_raw_parts(out_ptr as *const u8, out_len) };
    let value = json::parse(response).map_err(|_| String::from("host response"))?;
    if value.get("ok").and_then(json::Value::as_bool).unwrap_or(false) {
        Ok(value.get("result").cloned().unwrap_or(json::Value::Null))
    } else {
        // Surface the host's real error (e.g. "origin … not in http_origins")
        // instead of a generic label so guests can report it verbatim.
        Err(value
            .get("error")
            .and_then(json::Value::as_str)
            .map(String::from)
            .unwrap_or_else(|| String::from("host error")))
    }
}

pub fn kv_get(key: &str) -> Option<Vec<u8>> {
    let mut p = json::Value::object();
    p.insert("key", json::Value::str(key));
    let r = host("kv.get", p).ok()?;
    r.get("value").and_then(json::Value::as_str).map(|s| s.as_bytes().to_vec())
}
pub fn kv_set(key: &str, value: &str) {
    let mut p = json::Value::object();
    p.insert("key", json::Value::str(key));
    p.insert("value", json::Value::str(value));
    let _ = host("kv.set", p);
}
pub fn secret_get(key: &str) -> Option<Vec<u8>> {
    let mut p = json::Value::object();
    p.insert("key", json::Value::str(key));
    let r = host("secret.get", p).ok()?;
    r.get("value").and_then(json::Value::as_str).map(|s| s.as_bytes().to_vec())
}
pub fn secret_set(key: &str, value: &str) {
    let mut p = json::Value::object();
    p.insert("key", json::Value::str(key));
    p.insert("value", json::Value::str(value));
    let _ = host("secret.set", p);
}
pub fn http(url: &str, method: &str, body: Option<&str>) -> Result<json::Value, String> {
    let mut p = json::Value::object();
    p.insert("url", json::Value::str(url));
    p.insert("method", json::Value::str(method));
    if let Some(body) = body {
        p.insert("body", json::Value::str(body));
    }
    host("http.request", p)
}
pub fn log(message: &str) {
    let mut p = json::Value::object();
    p.insert("message", json::Value::str(message));
    let _ = host("log.write", p);
}

/// Minimal JSON value/parser/serializer sufficient for the plugin ABI.
pub mod json;