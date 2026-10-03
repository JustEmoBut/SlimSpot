//! "Start with Windows": a per-user Run registry value that launches the current exe into the tray.
//! The registry is the source of truth, so the setting reflects what Windows will actually do.

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::{PCWSTR, w};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: PCWSTR = w!("SlimSpot");
/// Command-line flag: start hidden in the tray.
pub const TRAY_ARG: &str = "--tray";

pub fn is_enabled() -> bool {
    let status = unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE_NAME, RRF_RT_REG_SZ, None, None, None) };
    status == ERROR_SUCCESS
}

pub fn set(enabled: bool) -> Result<(), String> {
    let status = if enabled {
        let exe = std::env::current_exe().map_err(|e| format!("Can't find own exe: {e}"))?;
        let command = format!("\"{}\" {TRAY_ARG}", exe.display());
        // REG_SZ data is UTF-16 including the terminating NUL; the size is in bytes.
        let data: Vec<u16> = command.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = (data.len() * size_of::<u16>()) as u32;
        unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE_NAME, REG_SZ.0, Some(data.as_ptr().cast()), bytes) }
    } else {
        match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE_NAME) } {
            ERROR_FILE_NOT_FOUND => ERROR_SUCCESS, // already off
            other => other,
        }
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("Changing autostart failed (Windows error {})", status.0))
    }
}
