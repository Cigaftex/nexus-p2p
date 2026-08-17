//! Small JSON-over-C ABI used by Flutter's `dart:ffi` bridge.

use std::{
    ffi::{CStr, CString},
    os::raw::c_char,
    ptr,
    sync::Arc,
};

use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::{runtime::Runtime, sync::broadcast};

use crate::{
    identity::{DeviceId, PublicIdentity},
    model::PeerEndpoint,
    node::{Node, NodeConfig, NodeEvent},
};

static LAST_ERROR: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

pub struct FfiHandle {
    runtime: Runtime,
    node: Arc<Node>,
    events: Mutex<broadcast::Receiver<NodeEvent>>,
}

impl Drop for FfiHandle {
    fn drop(&mut self) {
        let _ = self.runtime.block_on(self.node.shutdown());
    }
}

fn set_error(error: impl ToString) {
    if let Ok(mut value) = LAST_ERROR.lock() {
        *value = error.to_string();
    }
}

unsafe fn read_string<'a>(value: *const c_char) -> anyhow::Result<&'a str> {
    anyhow::ensure!(!value.is_null(), "null string pointer");
    Ok(CStr::from_ptr(value).to_str()?)
}

fn string_result(value: &Value) -> *mut c_char {
    CString::new(value.to_string())
        .map(CString::into_raw)
        .unwrap_or(ptr::null_mut())
}

#[no_mangle]
/// Creates a Nexus node from UTF-8 JSON.
///
/// # Safety
/// `config_json` must be a non-null, NUL-terminated string that remains valid
/// for the duration of this call. The returned handle must be released exactly
/// once with [`nexus_destroy`].
pub unsafe extern "C" fn nexus_create(config_json: *const c_char) -> *mut FfiHandle {
    let result = (|| -> anyhow::Result<*mut FfiHandle> {
        let config: NodeConfig = serde_json::from_str(read_string(config_json)?)?;
        let runtime = Runtime::new()?;
        let node = Node::new(config)?;
        let events = Mutex::new(node.subscribe());
        Ok(Box::into_raw(Box::new(FfiHandle {
            runtime,
            node,
            events,
        })))
    })();
    match result {
        Ok(handle) => handle,
        Err(error) => {
            set_error(error);
            ptr::null_mut()
        }
    }
}

#[derive(Deserialize)]
struct Command {
    op: String,
    #[serde(default)]
    peer_id: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    media_type: String,
    #[serde(default)]
    manifest_id: String,
    #[serde(default)]
    destination: String,
    #[serde(default)]
    identity: Option<PublicIdentity>,
    #[serde(default)]
    host: String,
    #[serde(default)]
    port: u16,
    #[serde(default)]
    display_name: String,
}

#[no_mangle]
/// Executes one JSON command and returns an owned UTF-8 JSON response.
///
/// # Safety
/// `handle` must come from [`nexus_create`] and remain live. `command_json` must
/// be a valid NUL-terminated string. Release the returned string exactly once
/// with [`nexus_string_free`].
pub unsafe extern "C" fn nexus_call(
    handle: *mut FfiHandle,
    command_json: *const c_char,
) -> *mut c_char {
    let result = (|| -> anyhow::Result<Value> {
        anyhow::ensure!(!handle.is_null(), "null Nexus handle");
        let handle = &*handle;
        let command: Command = serde_json::from_str(read_string(command_json)?)?;
        let peer_id = || DeviceId(command.peer_id.clone());
        let data = match command.op.as_str() {
            "start" => {
                handle.runtime.block_on(handle.node.start())?;
                json!({"identity": handle.node.identity()})
            }
            "identity" => serde_json::to_value(handle.node.identity())?,
            "set_display_name" => {
                serde_json::to_value(handle.node.set_display_name(&command.display_name)?)?
            }
            "peers" => serde_json::to_value(handle.node.peers()?)?,
            "remember_peer" => {
                let identity = command
                    .identity
                    .ok_or_else(|| anyhow::anyhow!("missing peer identity"))?;
                anyhow::ensure!(!command.host.trim().is_empty(), "missing peer host");
                anyhow::ensure!(command.port != 0, "missing peer port");
                handle.node.remember_peer(
                    identity,
                    PeerEndpoint {
                        host: command.host,
                        port: command.port,
                    },
                )?;
                Value::Null
            }
            "pair" => {
                handle.runtime.block_on(handle.node.pair(&peer_id()))?;
                Value::Null
            }
            "send_text" => serde_json::to_value(
                handle
                    .runtime
                    .block_on(handle.node.send_text(&peer_id(), &command.text))?,
            )?,
            "send_file" => {
                serde_json::to_value(handle.runtime.block_on(handle.node.send_file(
                    &peer_id(),
                    &command.path,
                    &command.media_type,
                ))?)?
            }
            "chat" => serde_json::to_value(handle.node.chat(&peer_id())?)?,
            "sync" => {
                handle.runtime.block_on(handle.node.sync(&peer_id()))?;
                Value::Null
            }
            "materialize_file" => {
                handle
                    .node
                    .materialize_file(&command.manifest_id, &command.destination)?;
                Value::Null
            }
            "events" => {
                let mut receiver = handle.events.lock();
                let mut events = Vec::new();
                while let Ok(event) = receiver.try_recv() {
                    events.push(event);
                }
                serde_json::to_value(events)?
            }
            _ => anyhow::bail!("unknown operation {}", command.op),
        };
        Ok(json!({"ok": true, "data": data}))
    })();
    match result {
        Ok(value) => string_result(&value),
        Err(error) => string_result(&json!({"ok": false, "error": error.to_string()})),
    }
}

#[no_mangle]
/// Returns an owned copy of the most recent creation error.
///
/// # Safety
/// Release the returned string exactly once with [`nexus_string_free`].
pub unsafe extern "C" fn nexus_last_error() -> *mut c_char {
    let error = LAST_ERROR
        .lock()
        .map(|value| value.clone())
        .unwrap_or_else(|_| "error lock poisoned".into());
    CString::new(error)
        .map(CString::into_raw)
        .unwrap_or(ptr::null_mut())
}

#[no_mangle]
/// Releases a string allocated by this library.
///
/// # Safety
/// `value` must be null or a pointer returned by `nexus_call` or
/// `nexus_last_error`, and it must not have been released previously.
pub unsafe extern "C" fn nexus_string_free(value: *mut c_char) {
    if !value.is_null() {
        drop(CString::from_raw(value));
    }
}

#[no_mangle]
/// Stops and releases a Nexus node.
///
/// # Safety
/// `handle` must be null or a live pointer returned by [`nexus_create`], and it
/// must not be used or released again after this call.
pub unsafe extern "C" fn nexus_destroy(handle: *mut FfiHandle) {
    if !handle.is_null() {
        drop(Box::from_raw(handle));
    }
}
