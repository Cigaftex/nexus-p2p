use std::ffi::{CStr, CString};

#[test]
fn json_c_abi_creates_calls_and_destroys_node() -> anyhow::Result<()> {
    let data = tempfile::tempdir()?;
    let config = CString::new(
        serde_json::json!({
            "data_dir": data.path(),
            "display_name": "FFI device",
            "listen_port": 0,
            "enable_mdns": false
        })
        .to_string(),
    )?;
    let command = CString::new(r#"{"op":"identity"}"#)?;
    let rename = CString::new(r#"{"op":"set_display_name","display_name":"Renamed device"}"#)?;
    unsafe {
        let handle = nexus_core::ffi::nexus_create(config.as_ptr());
        assert!(!handle.is_null());
        let response = nexus_core::ffi::nexus_call(handle, command.as_ptr());
        assert!(!response.is_null());
        let value: serde_json::Value = serde_json::from_str(CStr::from_ptr(response).to_str()?)?;
        assert_eq!(value["ok"], true);
        assert_eq!(value["data"]["display_name"], "FFI device");
        nexus_core::ffi::nexus_string_free(response);

        let response = nexus_core::ffi::nexus_call(handle, rename.as_ptr());
        let value: serde_json::Value = serde_json::from_str(CStr::from_ptr(response).to_str()?)?;
        assert_eq!(value["data"]["display_name"], "Renamed device");
        nexus_core::ffi::nexus_string_free(response);
        nexus_core::ffi::nexus_destroy(handle);

        let handle = nexus_core::ffi::nexus_create(config.as_ptr());
        let response = nexus_core::ffi::nexus_call(handle, command.as_ptr());
        let value: serde_json::Value = serde_json::from_str(CStr::from_ptr(response).to_str()?)?;
        assert_eq!(value["data"]["display_name"], "Renamed device");
        nexus_core::ffi::nexus_string_free(response);
        nexus_core::ffi::nexus_destroy(handle);
    }
    Ok(())
}
