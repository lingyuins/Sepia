//! WASM plugin runtime: manifest/package validation, sandboxed wasmtime
//! execution, host capabilities, and the inference/admin hook surface.
//!
//! Plugins are core wasm modules exporting `handle` (and optionally `alloc`).
//! The host passes a JSON event as `{"event":..,"payload":..,"config":..}` and
//! reads back a JSON result. A plugin may call the imported `host_call` to use
//! kv/secret/http/log capabilities, permission-checked per manifest. Execution
//! is bounded by fuel (work) and an epoch deadline (wall clock); linear memory
//! is capped. No WASI is provided, so outbound network only flows through the
//! audited host http capability.

use crate::{KeyStore, Store, store::PluginRecord};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use wasmtime::{Caller, Engine, Extern, Linker, Module, Store as WasmStore, StoreLimits};

pub const HOST_API: u64 = 1;
pub const MAX_PACKAGE_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_WASM_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_UI_FILE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_UI_FILES: usize = 128;
pub const MAX_IO_BYTES: usize = 1024 * 1024;
pub const DEFAULT_TIMEOUT_MS: u64 = 100;
pub const MAX_TIMEOUT_MS: u64 = 2000;
pub const DEFAULT_MEMORY_MIB: u64 = 16;
pub const MAX_MEMORY_MIB: u64 = 64;
pub const FUEL_PER_MS: u64 = 200_000;
const EPOCH_TICK_MS: u64 = 10;

pub const EVENTS: &[&str] = &[
    "relay.request.before_route",
    "relay.route.filter",
    "relay.request.before_upstream",
    "relay.response.before_send",
    "relay.completed",
    "relay.strategy",
    "provider.fetch",
    "provider.models",
    "plugin.migrate",
    "http.rpc",
];
pub const PERMISSIONS: &[&str] = &[
    "kv",
    "secret",
    "http",
    "log",
    "relay:read",
    "relay:write",
    "provider",
    "rpc:read",
    "rpc:write",
    "admin",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default = "default_host_api")]
    pub host_api: u64,
    #[serde(default = "default_entry")]
    pub entry: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub http_origins: Vec<String>,
    #[serde(default)]
    pub priority: i64,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default = "default_memory")]
    pub memory_mib: u64,
    #[serde(default)]
    pub config_schema: Value,
    #[serde(default)]
    pub ui: Option<UiManifest>,
}
fn default_host_api() -> u64 {
    HOST_API
}
fn default_entry() -> String {
    "handle".into()
}
fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_MS
}
fn default_memory() -> u64 {
    DEFAULT_MEMORY_MIB
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiManifest {
    #[serde(default = "default_ui_entry")]
    pub entry: String,
    #[serde(default)]
    pub title: String,
}
fn default_ui_entry() -> String {
    "index.html".into()
}

impl Manifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.host_api != HOST_API {
            return Err(format!("unsupported host_api {}", self.host_api));
        }
        if !valid_id(&self.id) {
            return Err("plugin id must match ^[a-z][a-z0-9-]{1,63}$".into());
        }
        if self.name.trim().is_empty() {
            return Err("plugin name required".into());
        }
        semver::Version::parse(&self.version).map_err(|_| "invalid semver version")?;
        if self.entry.trim().is_empty() {
            return Err("entry required".into());
        }
        for event in &self.events {
            if !EVENTS.contains(&event.as_str()) {
                return Err(format!("unknown event {event}"));
            }
        }
        for permission in &self.permissions {
            if !PERMISSIONS.contains(&permission.as_str()) {
                return Err(format!("unknown permission {permission}"));
            }
        }
        for origin in &self.http_origins {
            if url::Url::parse(origin).is_err() {
                return Err(format!("invalid http_origin {origin}"));
            }
        }
        if self.timeout_ms == 0 || self.timeout_ms > MAX_TIMEOUT_MS {
            return Err(format!("timeout_ms must be 1..={MAX_TIMEOUT_MS}"));
        }
        if self.memory_mib == 0 || self.memory_mib > MAX_MEMORY_MIB {
            return Err(format!("memory_mib must be 1..={MAX_MEMORY_MIB}"));
        }
        if self
            .events
            .iter()
            .any(|e| e.starts_with("relay.") && e != "relay.completed")
            && !self.permissions.iter().any(|p| p == "relay:write")
        {
            return Err("relay rewrite events require relay:write permission".into());
        }
        if self.events.iter().any(|e| e.starts_with("provider."))
            && !self.permissions.iter().any(|p| p == "provider")
        {
            return Err("provider events require provider permission".into());
        }
        Ok(())
    }
    pub fn has_event(&self, event: &str) -> bool {
        self.events.iter().any(|item| item == event)
    }
    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions.iter().any(|item| item == permission)
    }
}
fn valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    if bytes.len() < 2 || bytes.len() > 64 || !bytes[0].is_ascii_lowercase() {
        return false;
    }
    bytes
        .iter()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

pub struct Package {
    pub manifest: Manifest,
    pub wasm: Vec<u8>,
    pub ui: HashMap<String, Vec<u8>>,
    pub sha256: String,
}

pub fn parse_package(bytes: &[u8]) -> Result<Package, String> {
    use sha2::{Digest, Sha256};
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err("package exceeds 20 MiB".into());
    }
    let sha256 = hex::encode(Sha256::digest(bytes));
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("invalid zip: {e}"))?;
    let mut manifest: Option<Manifest> = None;
    let mut wasm: Option<Vec<u8>> = None;
    let mut ui: HashMap<String, Vec<u8>> = HashMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|e| e.to_string())?;
        if file.is_dir() {
            continue;
        }
        let name = file.name().replace('\\', "/");
        if name.contains("..") || name.starts_with('/') {
            return Err(format!("unsafe package path {name}"));
        }
        if name == "manifest.json" {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut file, &mut text).map_err(|e| e.to_string())?;
            manifest = Some(serde_json::from_str(&text).map_err(|e| format!("manifest: {e}"))?);
            continue;
        }
        if name == "plugin.wasm" {
            let mut buffer = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut buffer).map_err(|e| e.to_string())?;
            if buffer.len() > MAX_WASM_BYTES {
                return Err("plugin.wasm exceeds 16 MiB".into());
            }
            if !buffer.starts_with(b"\0asm") {
                return Err("plugin.wasm is not a wasm module".into());
            }
            wasm = Some(buffer);
            continue;
        }
        if let Some(relative) = name.strip_prefix("ui/") {
            if relative.is_empty() {
                return Err("empty ui path".into());
            }
            if ui.len() >= MAX_UI_FILES {
                return Err("ui exceeds 128 files".into());
            }
            let mut buffer = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut buffer).map_err(|e| e.to_string())?;
            if buffer.len() > MAX_UI_FILE_BYTES {
                return Err(format!("ui file {relative} exceeds 2 MiB"));
            }
            ui.insert(relative.to_string(), buffer);
            continue;
        }
        return Err(format!("unexpected package entry {name}"));
    }
    let manifest = manifest.ok_or("manifest.json missing")?;
    manifest.validate()?;
    let wasm = wasm.ok_or("plugin.wasm missing")?;
    if let Some(ui_manifest) = &manifest.ui
        && !ui.contains_key(&ui_manifest.entry)
    {
        return Err(format!("ui entry {} missing", ui_manifest.entry));
    }
    Ok(Package {
        manifest,
        wasm,
        ui,
        sha256,
    })
}

/// Build the sandboxed iframe document: assets become data URLs, scripts run
/// with a one-time session token and a strict CSP that forbids direct network.
pub fn ui_document(record: &PluginRecord, session: &str) -> Result<String, String> {
    let Some(ui_manifest) = &record.manifest.ui else {
        return Err("plugin has no ui".into());
    };
    let bundle = record.ui_bundle.as_object().ok_or("invalid ui bundle")?;
    let entry_encoded = bundle
        .get(&ui_manifest.entry)
        .and_then(Value::as_str)
        .ok_or("ui entry missing")?;
    use base64::Engine;
    let entry_bytes = base64::engine::general_purpose::STANDARD
        .decode(entry_encoded)
        .map_err(|_| "ui entry is not valid base64".to_string())?;
    let mut document =
        String::from_utf8(entry_bytes).map_err(|_| "ui entry is not utf-8".to_string())?;
    for (path, encoded) in bundle {
        if path == &ui_manifest.entry {
            continue;
        }
        let mime = mime_for(path);
        if mime == "text/javascript" {
            // Data-URL scripts are blocked by Chromium in opaque-origin
            // (sandboxed) frames, so inline JS assets directly instead.
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded.as_str().unwrap_or_default())
                .map_err(|_| format!("ui asset {path} is not valid base64"))?;
            let text = String::from_utf8(bytes)
                .map_err(|_| format!("ui asset {path} is not utf-8"))?;
            let inline = format!("<script>{text}</script>");
            document = document.replace(&format!("<script src=\"{path}\"></script>"), &inline);
            document = document.replace(&format!("<script src='{path}'></script>"), &inline);
            continue;
        }
        let encoded = encoded.as_str().unwrap_or_default();
        let data_url = format!("data:{mime};base64,{encoded}");
        document = document.replace(&format!("\"{path}\""), &format!("\"{data_url}\""));
        document = document.replace(&format!("'{path}'"), &format!("'{data_url}'"));
        document = document.replace(&format!("({path})"), &format!("({data_url})"));
    }
    let bridge = format!(
        "<script>window.__SEPIA_SESSION__={session:?};window.sepia={{call:function(method,params){{return new Promise(function(resolve,reject){{var id=Math.random().toString(36).slice(2);function onMessage(event){{if(event.source!==window.parent)return;var data=event.data;if(!data||data.id!==id||data.session!==window.__SEPIA_SESSION__)return;window.removeEventListener('message',onMessage);if(data.error)reject(new Error(data.error));else resolve(data.result);}}window.addEventListener('message',onMessage);window.parent.postMessage({{id:id,session:window.__SEPIA_SESSION__,method:method,params:params||{{}}}},'*');}});}}}};</script>"
    );
    let csp = "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; connect-src 'none'; font-src data:;\">";
    let document = if document.contains("<head>") {
        document.replacen("<head>", &format!("<head>{csp}{bridge}"), 1)
    } else {
        format!("{csp}{bridge}{document}")
    };
    Ok(document)
}
fn mime_for(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

pub struct PluginRuntime {
    engine: Engine,
    modules: HashMap<String, Module>,
}
impl PluginRuntime {
    pub fn new() -> Result<Self, String> {
        let mut config = wasmtime::Config::new();
        config.consume_fuel(true);
        config.epoch_interruption(true);
        config.wasm_threads(false);
        let engine = Engine::new(&config).map_err(|e| e.to_string())?;
        Ok(Self {
            engine,
            modules: HashMap::new(),
        })
    }
    pub async fn reload(&mut self, store: &Store) -> Result<(), String> {
        self.modules.clear();
        for record in store.plugin_records().await? {
            if record.status == "uninstalling" {
                continue;
            }
            let module = Module::new(&self.engine, &record.wasm)
                .map_err(|e| format!("plugin {}: {e}", record.id))?;
            self.modules.insert(record.id, module);
        }
        Ok(())
    }
    pub fn spawn_epoch_ticker(&self) -> tokio::task::JoinHandle<()> {
        let engine = self.engine.clone();
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_millis(EPOCH_TICK_MS));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                engine.increment_epoch();
            }
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn invoke(
        &self,
        store: &Store,
        keys: &KeyStore,
        http: &reqwest::Client,
        record: &PluginRecord,
        event: &str,
        payload: Value,
        admin: Option<AdminContext>,
    ) -> Result<Value, String> {
        let module = self
            .modules
            .get(&record.id)
            .ok_or_else(|| format!("plugin {} not loaded", record.id))?;
        let manifest = record.manifest.clone();
        let config = record.config.clone();
        let plugin_id = record.id.clone();
        let entry = manifest.entry.clone();
        let timeout_ms = manifest.timeout_ms;
        let memory_bytes = (manifest.memory_mib as usize) * 1024 * 1024;
        let input = json!({"event": event, "payload": payload, "config": config});
        let input_bytes = serde_json::to_vec(&input).map_err(|e| e.to_string())?;
        if input_bytes.len() > MAX_IO_BYTES {
            return Err("plugin input exceeds 1 MiB".into());
        }
        let host = HostState {
            store: store.clone(),
            keys: keys.clone(),
            http: http.clone(),
            plugin_id,
            manifest,
            input: input_bytes,
            logs: Vec::new(),
            limits: wasmtime::StoreLimitsBuilder::new().build(),
            runtime: tokio::runtime::Handle::current(),
            admin,
        };
        let engine = self.engine.clone();
        let module = module.clone();
        let result = tokio::task::spawn_blocking(move || -> Result<Value, String> {
            let mut wasm_store = WasmStore::new(&engine, host);
            wasm_store.limiter(|state| &mut state.limits);
            let limits = wasmtime::StoreLimitsBuilder::new()
                .memory_size(memory_bytes)
                .instances(1)
                .tables(1)
                .memories(1)
                .build();
            wasm_store.data_mut().limits = limits;
            wasm_store
                .set_fuel(timeout_ms * FUEL_PER_MS)
                .map_err(|e| e.to_string())?;
            wasm_store.set_epoch_deadline((timeout_ms / EPOCH_TICK_MS).max(1) + 1);
            wasm_store.epoch_deadline_trap();
            let mut linker: Linker<HostState> = Linker::new(&engine);
            linker
                .func_wrap("env", "host_call", host_call)
                .map_err(|e| e.to_string())?;
            let instance = linker
                .instantiate(&mut wasm_store, &module)
                .map_err(|e| e.to_string())?;
            let memory = instance
                .get_export(&mut wasm_store, "memory")
                .and_then(Extern::into_memory)
                .ok_or("plugin must export memory")?;
            let alloc = instance
                .get_export(&mut wasm_store, "alloc")
                .and_then(Extern::into_func);
            let grow = instance
                .get_export(&mut wasm_store, "sepia_grow")
                .and_then(Extern::into_func);
            let input_ptr = write_input(&mut wasm_store, &memory, alloc.as_ref(), grow.as_ref())?;
            let handle = instance
                .get_export(&mut wasm_store, &entry)
                .and_then(Extern::into_func)
                .ok_or_else(|| format!("plugin export {entry} missing"))?;
            let typed = handle
                .typed::<(i32, i32), i64>(&wasm_store)
                .map_err(|e| format!("handle signature: {e}"))?;
            let input_len = wasm_store.data().input.len() as i32;
            let packed = typed
                .call(&mut wasm_store, (input_ptr, input_len))
                .map_err(|e| format!("plugin trapped: {e}"))?;
            let output_ptr = (packed >> 32) as usize;
            let output_len = (packed & 0xffff_ffff) as usize;
            if output_len > MAX_IO_BYTES {
                return Err("plugin output exceeds 1 MiB".into());
            }
            let data = memory.data(&wasm_store);
            let slice = data
                .get(output_ptr..output_ptr + output_len)
                .ok_or("plugin output out of bounds")?;
            let value: Value =
                serde_json::from_slice(slice).map_err(|e| format!("plugin output json: {e}"))?;
            for line in &wasm_store.data().logs {
                tracing_log(&wasm_store.data().plugin_id, line);
            }
            Ok(value)
        })
        .await
        .map_err(|e| e.to_string())??;
        Ok(result)
    }
}
fn tracing_log(plugin_id: &str, line: &str) {
    eprintln!("[plugin {plugin_id}] {line}");
}
fn write_input(
    wasm_store: &mut WasmStore<HostState>,
    memory: &wasmtime::Memory,
    alloc: Option<&wasmtime::Func>,
    grow: Option<&wasmtime::Func>,
) -> Result<i32, String> {
    let input = wasm_store.data().input.clone();
    let len = input.len();
    let ptr = if let Some(alloc) = alloc {
        let typed = alloc
            .typed::<i32, i32>(&*wasm_store)
            .map_err(|e| format!("alloc signature: {e}"))?;
        typed
            .call(&mut *wasm_store, len as i32)
            .map_err(|e| format!("alloc trapped: {e}"))?
    } else {
        1024
    };
    let end = ptr as usize + len;
    let current = memory.data_size(&*wasm_store);
    if end > current {
        let needed_pages = (end - current).div_ceil(65536);
        if let Some(grow) = grow {
            let typed = grow
                .typed::<i32, i32>(&*wasm_store)
                .map_err(|e| format!("grow signature: {e}"))?;
            let previous = typed
                .call(&mut *wasm_store, needed_pages as i32)
                .map_err(|e| format!("grow trapped: {e}"))?;
            if previous < 0 {
                return Err("plugin memory growth failed".into());
            }
        }
        if end > memory.data_size(&*wasm_store) {
            return Err("plugin input does not fit in memory".into());
        }
    }
    let data = memory.data_mut(&mut *wasm_store);
    data[ptr as usize..end].copy_from_slice(&input);
    Ok(ptr)
}

struct HostState {
    store: Store,
    keys: KeyStore,
    http: reqwest::Client,
    plugin_id: String,
    manifest: Manifest,
    input: Vec<u8>,
    logs: Vec<String>,
    limits: StoreLimits,
    runtime: tokio::runtime::Handle,
    admin: Option<AdminContext>,
}
impl HostState {
    fn read_guest_string(
        caller: &mut Caller<'_, Self>,
        ptr: i32,
        len: i32,
    ) -> Result<String, String> {
        if len < 0 || len as usize > MAX_IO_BYTES {
            return Err("host_call payload too large".into());
        }
        let memory = caller
            .get_export("memory")
            .and_then(Extern::into_memory)
            .ok_or("memory missing")?;
        let data = memory.data(&*caller);
        let start = ptr as usize;
        let slice = data
            .get(start..start + len as usize)
            .ok_or("host_call payload out of bounds")?;
        String::from_utf8(slice.to_vec()).map_err(|e| e.to_string())
    }
    fn write_guest_bytes(caller: &mut Caller<'_, Self>, bytes: &[u8]) -> Result<i64, String> {
        if bytes.len() > MAX_IO_BYTES {
            return Err("host response exceeds 1 MiB".into());
        }
        let memory = caller
            .get_export("memory")
            .and_then(Extern::into_memory)
            .ok_or("memory missing")?;
        let alloc = caller.get_export("alloc").and_then(Extern::into_func);
        let ptr = if let Some(alloc) = alloc {
            let typed = alloc
                .typed::<i32, i32>(&*caller)
                .map_err(|e| e.to_string())?;
            typed
                .call(&mut *caller, bytes.len() as i32)
                .map_err(|e| e.to_string())?
        } else {
            let data_len = memory.data(&*caller).len();
            (data_len - bytes.len() - 16) as i32
        };
        let end = ptr as usize + bytes.len();
        let current = memory.data_size(&*caller);
        if end > current {
            let needed_pages = (end - current).div_ceil(65536);
            if let Some(grow) = caller.get_export("sepia_grow").and_then(Extern::into_func) {
                let typed = grow
                    .typed::<i32, i32>(&*caller)
                    .map_err(|e| e.to_string())?;
                let previous = typed
                    .call(&mut *caller, needed_pages as i32)
                    .map_err(|e| e.to_string())?;
                if previous < 0 {
                    return Err("plugin memory growth failed".into());
                }
            }
            if end > memory.data_size(&*caller) {
                return Err("host response does not fit".into());
            }
        }
        let data = memory.data_mut(&mut *caller);
        data[ptr as usize..end].copy_from_slice(bytes);
        Ok(((ptr as i64) << 32) | bytes.len() as i64)
    }
}

fn host_call(mut caller: Caller<'_, HostState>, ptr: i32, len: i32) -> i64 {
    let request = match HostState::read_guest_string(&mut caller, ptr, len) {
        Ok(text) => text,
        Err(error) => return error_response(&mut caller, &error),
    };
    let request: Value = match serde_json::from_str(&request) {
        Ok(value) => value,
        Err(error) => return error_response(&mut caller, &error.to_string()),
    };
    let capability = request
        .get("capability")
        .and_then(Value::as_str)
        .unwrap_or("");
    let payload = request.get("payload").cloned().unwrap_or(json!({}));
    let state = caller.data();
    let store = state.store.clone();
    let keys = state.keys.clone();
    let http = state.http.clone();
    let plugin_id = state.plugin_id.clone();
    let manifest = state.manifest.clone();
    let handle = state.runtime.clone();
    let outcome = if capability == "admin.request" {
        let admin = state.admin.clone();
        handle.block_on(host_admin(admin, &manifest, &http, payload))
    } else {
        handle.block_on(dispatch_capability(
            &store, &keys, &http, &plugin_id, &manifest, capability, payload,
        ))
    };
    let response = match outcome {
        Ok(value) => json!({"ok": true, "result": value}),
        Err(error) => json!({"ok": false, "error": error}),
    };
    let bytes = serde_json::to_vec(&response).unwrap_or_default();
    match HostState::write_guest_bytes(&mut caller, &bytes) {
        Ok(packed) => packed,
        Err(error) => error_response(&mut caller, &error),
    }
}
fn error_response(caller: &mut Caller<'_, HostState>, error: &str) -> i64 {
    let bytes = serde_json::to_vec(&json!({"ok": false, "error": error})).unwrap_or_default();
    HostState::write_guest_bytes(caller, &bytes).unwrap_or(0)
}

async fn dispatch_capability(
    store: &Store,
    keys: &KeyStore,
    http: &reqwest::Client,
    plugin_id: &str,
    manifest: &Manifest,
    capability: &str,
    payload: Value,
) -> Result<Value, String> {
    let required = match capability {
        "kv.get" | "kv.set" | "kv.delete" | "kv.keys" => "kv",
        "secret.get" | "secret.set" | "secret.delete" => "secret",
        "http.request" => "http",
        "log.write" => "log",
        _ => return Err(format!("unknown capability {capability}")),
    };
    if !manifest.has_permission(required) {
        return Err(format!("permission {required} not granted"));
    }
    let key = payload
        .get("key")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    match capability {
        "kv.get" => Ok(json!({ "value": store.plugin_kv_get(plugin_id, &key).await? })),
        "kv.set" => {
            let value = payload
                .get("value")
                .map(|value| value.to_string())
                .unwrap_or_default();
            store.plugin_kv_set(plugin_id, &key, &value).await?;
            Ok(json!({}))
        }
        "kv.delete" => {
            store.plugin_kv_delete(plugin_id, &key).await?;
            Ok(json!({}))
        }
        "kv.keys" => Ok(json!({ "keys": store.plugin_kv_keys(plugin_id).await? })),
        "secret.get" => {
            let value = match store.plugin_secret_get(plugin_id, &key).await? {
                Some((nonce, ciphertext)) => Some(keys.decrypt(&nonce, &ciphertext)?.to_string()),
                None => None,
            };
            Ok(json!({ "value": value }))
        }
        "secret.set" => {
            let value = payload
                .get("value")
                .and_then(Value::as_str)
                .ok_or("secret value required")?;
            let (nonce, ciphertext) = keys.encrypt(value)?;
            store
                .plugin_secret_set(plugin_id, &key, &nonce, &ciphertext)
                .await?;
            Ok(json!({}))
        }
        "secret.delete" => {
            store.plugin_secret_delete(plugin_id, &key).await?;
            Ok(json!({}))
        }
        "log.write" => {
            let message = payload.get("message").and_then(Value::as_str).unwrap_or("");
            eprintln!("[plugin {plugin_id}] {message}");
            Ok(json!({}))
        }
        "http.request" => host_http(http, manifest, payload).await,
        _ => Err(format!("unknown capability {capability}")),
    }
}
async fn host_http(
    http: &reqwest::Client,
    manifest: &Manifest,
    payload: Value,
) -> Result<Value, String> {
    let url = payload
        .get("url")
        .and_then(Value::as_str)
        .ok_or("http url required")?;
    let parsed = url::Url::parse(url).map_err(|e| e.to_string())?;
    let origin = format!(
        "{}://{}{}",
        parsed.scheme(),
        parsed.host_str().unwrap_or(""),
        parsed.port().map(|p| format!(":{p}")).unwrap_or_default()
    );
    if !manifest
        .http_origins
        .iter()
        .any(|allowed| allowed.trim_end_matches('/') == origin)
    {
        return Err(format!("origin {origin} not in http_origins"));
    }
    let method = payload
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("GET")
        .to_uppercase();
    let method = reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?;
    let mut builder = http.request(method, url);
    if let Some(headers) = payload.get("headers").and_then(Value::as_object) {
        for (name, value) in headers {
            if let Some(value) = value.as_str() {
                builder = builder.header(name.as_str(), value);
            }
        }
    }
    if let Some(body) = payload.get("body") {
        let text = match body {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        if text.len() > MAX_IO_BYTES {
            return Err("http body exceeds 1 MiB".into());
        }
        builder = builder.body(text);
    }
    let response = builder
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("http request failed: {e}"))?;
    let status = response.status().as_u16();
    let mut headers = serde_json::Map::new();
    for (name, value) in response.headers() {
        if let Ok(text) = value.to_str() {
            headers.insert(name.as_str().to_string(), json!(text));
        }
    }
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > MAX_IO_BYTES {
        return Err("http response exceeds 1 MiB".into());
    }
    let body = String::from_utf8_lossy(&bytes).to_string();
    Ok(json!({"status": status, "headers": headers, "body": body}))
}

/// Session credentials threaded from an admin-authenticated RPC invocation so a
/// plugin holding the `admin` permission can call the gateway management API
/// on behalf of the invoking administrator. Never available to non-RPC events.
#[derive(Clone)]
pub struct AdminContext {
    pub base_url: String,
    pub cookie: String,
}

async fn host_admin(
    admin: Option<AdminContext>,
    manifest: &Manifest,
    http: &reqwest::Client,
    payload: Value,
) -> Result<Value, String> {
    if !manifest.has_permission("admin") {
        return Err("permission admin not granted".into());
    }
    let admin = admin.ok_or("admin.request requires an admin session")?;
    let path = payload
        .get("path")
        .and_then(Value::as_str)
        .ok_or("admin path required")?;
    if !path.starts_with('/')
        || path[1..]
            .split('/')
            .any(|segment| segment.is_empty() || segment.chars().all(|character| character == '.'))
        || path.contains(['\\', '%', '?', '#'])
        || path.chars().any(char::is_control)
    {
        return Err("invalid admin path".into());
    }
    let method = payload
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("POST")
        .to_uppercase();
    let method = reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?;
    let url = format!("{}{}", admin.base_url.trim_end_matches('/'), path);
    let mut builder = http
        .request(method, &url)
        .header("cookie", &admin.cookie)
        .header("origin", &admin.base_url)
        .timeout(std::time::Duration::from_secs(30));
    if let Some(body) = payload.get("body") {
        let text = match body {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        if text.len() > MAX_IO_BYTES {
            return Err("admin body exceeds 1 MiB".into());
        }
        builder = builder
            .header("content-type", "application/json")
            .body(text);
    }
    let response = builder
        .send()
        .await
        .map_err(|e| format!("admin request failed: {e}"))?;
    let status = response.status().as_u16();
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > MAX_IO_BYTES {
        return Err("admin response exceeds 1 MiB".into());
    }
    let body = String::from_utf8_lossy(&bytes).to_string();
    Ok(json!({"status": status, "body": body}))
}

/// Shared, swappable runtime handle stored in AppState.
pub type SharedRuntime = Arc<tokio::sync::RwLock<PluginRuntime>>;
pub fn shared(runtime: PluginRuntime) -> SharedRuntime {
    Arc::new(tokio::sync::RwLock::new(runtime))
}

/// Enabled plugin records subscribing to an event, ordered by priority.
pub async fn subscribers(store: &Store, event: &str) -> Vec<PluginRecord> {
    let mut records: Vec<PluginRecord> = store
        .plugin_records()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|record| record.enabled && record.manifest.has_event(event))
        .collect();
    records.sort_by_key(|record| record.manifest.priority);
    records
}

/// Rewrite hooks fail closed: a plugin error aborts the request. The first
/// plugin returning `{"request": {...}}` / `{"response": {...}}` wins.
pub async fn rewrite_request(
    state: &crate::server::AppState,
    request: &crate::protocol::ChatRequest,
) -> Result<crate::protocol::ChatRequest, String> {
    let records = subscribers(&state.store, "relay.request.before_route").await;
    if records.is_empty() {
        return Ok(request.clone());
    }
    let mut current = request.clone();
    let runtime = state.plugins.read().await;
    for record in records {
        let payload = request_json(&current);
        let output = runtime
            .invoke(
                &state.store,
                &state.keys,
                &state.http(),
                &record,
                "relay.request.before_route",
                json!({"request": payload}),
                None,
            )
            .await
            .map_err(|error| format!("plugin {}: {error}", record.id))?;
        if let Some(value) = output.get("request") {
            current = apply_request(&current, value)?;
        }
    }
    Ok(current)
}

/// Route filter/strategy hooks may exclude or reorder candidates. Exclusions
/// fail closed; an empty result keeps the original candidates (fail open).
pub async fn filter_candidates(
    state: &crate::server::AppState,
    model: &str,
    candidates: &[(i64, String)],
) -> Result<Vec<(i64, String)>, String> {
    let mut current = candidates.to_vec();
    let runtime = state.plugins.read().await;
    for event in ["relay.route.filter", "relay.strategy"] {
        for record in subscribers(&state.store, event).await {
            let payload = json!({
                "model": model,
                "candidates": current
                    .iter()
                    .map(|(provider_id, model)| json!({"provider_id": provider_id, "model": model}))
                    .collect::<Vec<_>>(),
            });
            let output = runtime
                .invoke(
                    &state.store,
                    &state.keys,
                    &state.http(),
                    &record,
                    event,
                    payload,
                    None,
                )
                .await
                .map_err(|error| format!("plugin {}: {error}", record.id))?;
            if let Some(excluded) = output.get("exclude_channel_ids").and_then(Value::as_array) {
                let excluded: Vec<i64> = excluded.iter().filter_map(Value::as_i64).collect();
                current.retain(|(provider_id, _)| !excluded.contains(provider_id));
            }
            if let Some(ordered) = output.get("candidates").and_then(Value::as_array) {
                let mut reordered = Vec::new();
                for entry in ordered {
                    let provider_id = entry.get("provider_id").and_then(Value::as_i64);
                    let model = entry.get("model").and_then(Value::as_str);
                    if let (Some(provider_id), Some(model)) = (provider_id, model)
                        && let Some(found) = current
                            .iter()
                            .find(|(id, name)| *id == provider_id && name == model)
                    {
                        reordered.push(found.clone());
                    }
                }
                if !reordered.is_empty() {
                    current = reordered;
                }
            }
        }
    }
    if current.is_empty() {
        return Ok(candidates.to_vec());
    }
    Ok(current)
}

/// Upstream rewrite hooks adjust outbound headers/body. Sensitive headers are
/// never exposed to plugins nor writable by them.
pub async fn rewrite_upstream(
    state: &crate::server::AppState,
    provider: &crate::Provider,
    model: &str,
    body: &Value,
    headers: &[(String, String)],
) -> Result<(Value, Vec<(String, String)>), String> {
    let records = subscribers(&state.store, "relay.request.before_upstream").await;
    if records.is_empty() {
        return Ok((body.clone(), headers.to_vec()));
    }
    let mut current_body = body.clone();
    let mut current_headers = headers.to_vec();
    let runtime = state.plugins.read().await;
    for record in records {
        let visible_headers: Vec<Value> = current_headers
            .iter()
            .filter(|(name, _)| !is_sensitive_header(name))
            .map(|(name, value)| json!({"name": name, "value": value}))
            .collect();
        let output = runtime
            .invoke(
                &state.store,
                &state.keys,
                &state.http(),
                &record,
                "relay.request.before_upstream",
                json!({
                    "provider": {"id": provider.id, "name": provider.name, "protocol": provider.protocol.as_str()},
                    "model": model,
                    "body": current_body,
                    "headers": visible_headers,
                }),
                None,
            )
            .await
            .map_err(|error| format!("plugin {}: {error}", record.id))?;
        if let Some(value) = output.get("body") {
            current_body = value.clone();
        }
        if let Some(values) = output.get("headers").and_then(Value::as_array) {
            for entry in values {
                let name = entry.get("name").and_then(Value::as_str).unwrap_or("");
                let value = entry.get("value").and_then(Value::as_str).unwrap_or("");
                if name.is_empty() || is_sensitive_header(name) || !writable_header(name) {
                    continue;
                }
                current_headers.retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
                current_headers.push((name.to_string(), value.to_string()));
            }
        }
    }
    Ok((current_body, current_headers))
}
fn is_sensitive_header(name: &str) -> bool {
    const SENSITIVE: &[&str] = &[
        "authorization",
        "proxy-authorization",
        "cookie",
        "set-cookie",
        "host",
        "x-api-key",
        "x-goog-api-key",
        "anthropic-version",
        "content-length",
        "transfer-encoding",
        "connection",
        "keep-alive",
    ];
    SENSITIVE.iter().any(|item| item.eq_ignore_ascii_case(name))
}
fn writable_header(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("x-") || lower == "content-type" || lower == "accept"
}

/// Non-streaming response rewrite. The first plugin returning `{"response":
/// {...}}` wins; errors fail closed.
pub async fn rewrite_response(
    state: &crate::server::AppState,
    model: &str,
    response: &Value,
) -> Result<Value, String> {
    let records = subscribers(&state.store, "relay.response.before_send").await;
    if records.is_empty() {
        return Ok(response.clone());
    }
    let mut current = response.clone();
    let runtime = state.plugins.read().await;
    for record in records {
        let output = runtime
            .invoke(
                &state.store,
                &state.keys,
                &state.http(),
                &record,
                "relay.response.before_send",
                json!({"model": model, "response": current}),
                None,
            )
            .await
            .map_err(|error| format!("plugin {}: {error}", record.id))?;
        if let Some(value) = output.get("response") {
            current = value.clone();
        }
    }
    Ok(current)
}

/// Completion observation is fire-and-forget and never affects the request.
pub fn observe_completion(state: &crate::server::AppState, payload: Value) {
    let state = state.clone();
    tokio::spawn(async move {
        let records = subscribers(&state.store, "relay.completed").await;
        if records.is_empty() {
            return;
        }
        let runtime = state.plugins.read().await;
        for record in records {
            let _ = runtime
                .invoke(
                    &state.store,
                    &state.keys,
                    &state.http(),
                    &record,
                    "relay.completed",
                    payload.clone(),
                    None,
                )
                .await;
        }
    });
}

pub fn request_json(request: &crate::protocol::ChatRequest) -> Value {
    json!({
        "model": request.model,
        "system": request.system,
        "max_tokens": request.max_tokens,
        "temperature": request.temperature,
        "top_p": request.top_p,
        "stop": request.stop,
        "stream": request.stream,
        "messages": request.messages.iter().map(|message| {
            json!({"role": role_name(message.role), "text": message.text})
        }).collect::<Vec<_>>(),
        "passthrough": request.passthrough,
    })
}
fn role_name(role: crate::protocol::Role) -> &'static str {
    match role {
        crate::protocol::Role::System => "system",
        crate::protocol::Role::User => "user",
        crate::protocol::Role::Assistant => "assistant",
        crate::protocol::Role::Tool => "tool",
    }
}
pub fn apply_request(
    base: &crate::protocol::ChatRequest,
    value: &Value,
) -> Result<crate::protocol::ChatRequest, String> {
    let mut request = base.clone();
    if let Some(model) = value.get("model").and_then(Value::as_str) {
        request.model = model.to_string();
    }
    if let Some(system) = value.get("system") {
        request.system = system.as_str().map(str::to_string);
    }
    if let Some(max_tokens) = value.get("max_tokens").and_then(Value::as_u64) {
        request.max_tokens = Some(max_tokens as u32);
    }
    if let Some(temperature) = value.get("temperature").and_then(Value::as_f64) {
        request.temperature = Some(temperature);
    }
    if let Some(top_p) = value.get("top_p").and_then(Value::as_f64) {
        request.top_p = Some(top_p);
    }
    if let Some(stop) = value.get("stop").and_then(Value::as_array) {
        request.stop = stop
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
    }
    if let Some(stream) = value.get("stream").and_then(Value::as_bool) {
        request.stream = stream;
    }
    if let Some(passthrough) = value.get("passthrough").and_then(Value::as_object) {
        for (key, value) in passthrough {
            request.passthrough.insert(key.clone(), value.clone());
        }
    }
    if let Some(messages) = value.get("messages").and_then(Value::as_array) {
        let mut rebuilt = Vec::new();
        for message in messages {
            let role = match message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("user")
            {
                "system" => crate::protocol::Role::System,
                "assistant" => crate::protocol::Role::Assistant,
                "tool" => crate::protocol::Role::Tool,
                _ => crate::protocol::Role::User,
            };
            let text = message
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            rebuilt.push(crate::protocol::Message::new(role, text));
        }
        if !rebuilt.is_empty() {
            request.messages = rebuilt;
        }
    }
    Ok(request)
}

/// A provider backed by a plugin delegates model discovery to that plugin's
/// `provider.models` event. The plugin receives the provider descriptor and the
/// decrypted upstream secret; it returns `{"models": [...]}`.
pub async fn provider_models(
    state: &crate::server::AppState,
    provider: &crate::Provider,
    secret: &str,
) -> Result<Vec<String>, String> {
    let plugin_id = provider
        .plugin_id
        .clone()
        .ok_or("provider is not plugin-backed")?;
    let record = state
        .store
        .plugin(&plugin_id)
        .await?
        .ok_or_else(|| format!("plugin {plugin_id} not found"))?;
    if !record.enabled {
        return Err(format!("plugin {plugin_id} is disabled"));
    }
    if !record.manifest.has_event("provider.models") {
        return Err(format!(
            "plugin {plugin_id} does not handle provider.models"
        ));
    }
    let runtime = state.plugins.read().await;
    let output = runtime
        .invoke(
            &state.store,
            &state.keys,
            &state.http(),
            &record,
            "provider.models",
            json!({
                "provider": provider_descriptor(provider),
                "secret": secret,
            }),
            None,
        )
        .await
        .map_err(|error| format!("plugin {}: {error}", record.id))?;
    let models = output
        .get("models")
        .and_then(Value::as_array)
        .ok_or("plugin returned no models")?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    if models.is_empty() {
        return Err("plugin returned no models".into());
    }
    Ok(models)
}

/// A plugin-backed provider produces the raw upstream-protocol response through
/// its `provider.fetch` event. The returned bytes are decoded by the normal
/// protocol pipeline so streaming/failover/observability stay consistent.
pub async fn provider_fetch(
    state: &crate::server::AppState,
    provider: &crate::Provider,
    secret: &str,
    request: &crate::protocol::ChatRequest,
    model: &str,
    protocol: crate::protocol::Protocol,
) -> Result<reqwest::Response, String> {
    let plugin_id = provider
        .plugin_id
        .clone()
        .ok_or("provider is not plugin-backed")?;
    let record = state
        .store
        .plugin(&plugin_id)
        .await?
        .ok_or_else(|| format!("plugin {plugin_id} not found"))?;
    if !record.enabled {
        return Err(format!("plugin {plugin_id} is disabled"));
    }
    if !record.manifest.has_event("provider.fetch") {
        return Err(format!("plugin {plugin_id} does not handle provider.fetch"));
    }
    let runtime = state.plugins.read().await;
    let output = runtime
        .invoke(
            &state.store,
            &state.keys,
            &state.http(),
            &record,
            "provider.fetch",
            json!({
                "provider": provider_descriptor(provider),
                "secret": secret,
                "model": model,
                "protocol": protocol_label(protocol),
                "stream": request.stream,
                "request": request_json(request),
            }),
            None,
        )
        .await
        .map_err(|error| format!("plugin {}: {error}", record.id))?;
    let status = output.get("status").and_then(Value::as_u64).unwrap_or(200) as u16;
    let body = match output.get("body") {
        Some(Value::String(text)) => text.clone(),
        Some(value) => value.to_string(),
        None => "{}".to_string(),
    };
    let mut builder = axum::http::Response::builder().status(status);
    if let Some(headers) = output.get("headers").and_then(Value::as_object) {
        for (name, value) in headers {
            if let Some(value) = value.as_str() {
                builder = builder.header(name.as_str(), value);
            }
        }
    }
    builder = builder.header("content-type", "application/json");
    builder
        .body(body)
        .map(reqwest::Response::from)
        .map_err(|error| format!("plugin response: {error}"))
}

fn provider_descriptor(provider: &crate::Provider) -> Value {
    json!({
        "id": provider.id,
        "name": provider.name,
        "protocol": provider.protocol.as_str(),
        "endpoint": provider.endpoint,
        "models": provider.models,
    })
}
fn protocol_label(protocol: crate::protocol::Protocol) -> &'static str {
    match protocol {
        crate::protocol::Protocol::OpenAI => "openai",
        crate::protocol::Protocol::Anthropic => "anthropic",
        crate::protocol::Protocol::Gemini => "gemini",
        crate::protocol::Protocol::OpenAIResponses => "openai-responses",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_validation_rejects_bad_ids_and_permissions() {
        let mut manifest = Manifest {
            id: "demo".into(),
            name: "Demo".into(),
            version: "1.0.0".into(),
            host_api: HOST_API,
            entry: "handle".into(),
            description: String::new(),
            events: vec!["relay.request.before_route".into()],
            permissions: vec![],
            http_origins: vec![],
            priority: 0,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            memory_mib: DEFAULT_MEMORY_MIB,
            config_schema: json!({}),
            ui: None,
        };
        assert!(manifest.validate().is_err());
        manifest.permissions.push("relay:write".into());
        assert!(manifest.validate().is_ok());
        manifest.id = "Bad".into();
        assert!(manifest.validate().is_err());
    }
    #[test]
    fn package_requires_wasm_magic_and_manifest() {
        assert!(parse_package(b"not a zip").is_err());
    }
}
