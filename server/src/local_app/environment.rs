//! Keeps the host's `NODE_EXTRA_CA_CERTS` pointing at the local CA.
//!
//! Cursor's Electron shell trusts the system root store for Chromium requests,
//! but its Node side reads `NODE_EXTRA_CA_CERTS` and nothing else. Without that
//! variable those requests fail with "self signed certificate in certificate
//! chain" and the UI stays disconnected even while the proxy is running.
//!
//! The value is owned: the first write journals what was there before, and
//! clearing restores it only while the current value is still ours.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{config::managed_data_dir, Error, Result};

const ENV_NAME: &str = "NODE_EXTRA_CA_CERTS";

#[derive(Debug, Default, Serialize, Deserialize)]
struct Snapshot {
    #[serde(default)]
    present: bool,
    #[serde(default)]
    value: String,
    #[serde(default)]
    applied: String,
}

fn snapshot_value(snapshot: &Snapshot) -> Option<String> {
    snapshot.present.then(|| snapshot.value.clone())
}

fn snapshot_path() -> Result<PathBuf> {
    let directory = managed_data_dir()?.join("ca");
    fs::create_dir_all(&directory)?;
    Ok(directory.join("node-extra-ca-certs.json"))
}

fn load_snapshot() -> Result<Option<Snapshot>> {
    let path = snapshot_path()?;
    match fs::read(&path) {
        Ok(data) => serde_json::from_slice(&data)
            .map(Some)
            .map_err(|error| Error::Config(format!("parse {}: {error}", path.display()))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn store_snapshot(snapshot: &Snapshot) -> Result<()> {
    let path = snapshot_path()?;
    let data = serde_json::to_vec_pretty(snapshot)
        .map_err(|error| Error::Config(format!("encode host environment snapshot: {error}")))?;
    fs::write(&path, data)?;
    Ok(())
}

fn remove_snapshot() -> Result<()> {
    match fs::remove_file(snapshot_path()?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(any(windows, target_os = "macos"))]
pub fn ensure(cert_path: &Path) -> Result<bool> {
    let applied = cert_path
        .to_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Config("CA certificate path is empty".into()))?
        .to_owned();
    let current = user_value()?;
    let snapshot = load_snapshot()?;

    if let Some(snapshot) = snapshot.as_ref() {
        // Another tool changed the variable after we did: leave its value alone
        // instead of quietly taking ownership of it.
        if current.as_deref() != Some(applied.as_str()) && current != snapshot_value(snapshot) {
            tracing::warn!(
                current = current.as_deref().unwrap_or(""),
                "NODE_EXTRA_CA_CERTS was changed outside cursor-byok; leaving it alone"
            );
            return Ok(false);
        }
    }

    std::env::set_var(ENV_NAME, &applied);
    if current.as_deref() == Some(applied.as_str()) {
        return Ok(false);
    }

    let mut snapshot = snapshot.unwrap_or_else(|| Snapshot {
        present: current.is_some(),
        value: current.clone().unwrap_or_default(),
        applied: String::new(),
    });
    snapshot.applied = applied.clone();
    // Journal before mutating: if the user-level write fails, clearing still
    // knows what the original value was.
    store_snapshot(&snapshot)?;
    set_user_value(Some(&applied))?;
    Ok(true)
}

#[cfg(any(windows, target_os = "macos"))]
pub fn clear() -> Result<()> {
    let Some(snapshot) = load_snapshot()? else {
        std::env::remove_var(ENV_NAME);
        return Ok(());
    };
    let current = user_value()?;
    if current.as_deref() == Some(snapshot.applied.as_str()) {
        set_user_value(snapshot_value(&snapshot).as_deref())?;
    }
    match snapshot_value(&snapshot) {
        Some(value) => std::env::set_var(ENV_NAME, value),
        None => std::env::remove_var(ENV_NAME),
    }
    remove_snapshot()
}

/// Platforms without a user-level environment for GUI processes can only pass
/// the variable down to children of this server.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn ensure(cert_path: &Path) -> Result<bool> {
    let applied = cert_path
        .to_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Config("CA certificate path is empty".into()))?;
    std::env::set_var(ENV_NAME, applied);
    Ok(false)
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn clear() -> Result<()> {
    std::env::remove_var(ENV_NAME);
    Ok(())
}

#[cfg(windows)]
fn user_value() -> Result<Option<String>> {
    win::read_value()
}

#[cfg(windows)]
fn set_user_value(value: Option<&str>) -> Result<()> {
    win::write_value(value)
}

#[cfg(target_os = "macos")]
fn user_value() -> Result<Option<String>> {
    let output = std::process::Command::new("launchctl")
        .args(["getenv", ENV_NAME])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok((!value.is_empty()).then_some(value))
}

#[cfg(target_os = "macos")]
fn set_user_value(value: Option<&str>) -> Result<()> {
    let mut command = std::process::Command::new("launchctl");
    match value {
        Some(value) => {
            command.args(["setenv", ENV_NAME, value]);
        }
        None => {
            command.args(["unsetenv", ENV_NAME]);
        }
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(Error::Config(format!(
            "launchctl {} failed: {}",
            if value.is_some() {
                "setenv"
            } else {
                "unsetenv"
            },
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

#[cfg(windows)]
mod win {
    use std::io;

    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
            HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ,
            REG_VALUE_TYPE,
        },
        UI::WindowsAndMessaging::{
            SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
        },
    };

    use crate::{Error, Result};

    use super::ENV_NAME;

    const ENVIRONMENT_KEY: &str = "Environment";

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn open_key(access: u32) -> Result<HKEY> {
        let subkey = wide(ENVIRONMENT_KEY);
        let mut key: HKEY = std::ptr::null_mut();
        let status =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, access, &mut key) };
        if status != ERROR_SUCCESS {
            return Err(Error::Config(format!(
                "open HKCU\\{ENVIRONMENT_KEY}: {}",
                io::Error::from_raw_os_error(status as i32)
            )));
        }
        Ok(key)
    }

    pub(super) fn read_value() -> Result<Option<String>> {
        let key = open_key(KEY_QUERY_VALUE)?;
        let name = wide(ENV_NAME);
        let mut kind: REG_VALUE_TYPE = 0;
        let mut size = 0u32;
        let status = unsafe {
            RegQueryValueExW(
                key,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            unsafe { RegCloseKey(key) };
            return Ok(None);
        }
        if status != ERROR_SUCCESS {
            unsafe { RegCloseKey(key) };
            return Err(Error::Config(format!(
                "query HKCU\\{ENVIRONMENT_KEY}\\{ENV_NAME}: {}",
                io::Error::from_raw_os_error(status as i32)
            )));
        }
        if kind != REG_SZ && kind != REG_EXPAND_SZ {
            unsafe { RegCloseKey(key) };
            return Err(Error::Config(format!(
                "HKCU\\{ENVIRONMENT_KEY}\\{ENV_NAME} has unsupported registry type {kind}"
            )));
        }

        let mut data = vec![0u8; size as usize];
        let status = unsafe {
            RegQueryValueExW(
                key,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                data.as_mut_ptr(),
                &mut size,
            )
        };
        unsafe { RegCloseKey(key) };
        if status != ERROR_SUCCESS {
            return Err(Error::Config(format!(
                "read HKCU\\{ENVIRONMENT_KEY}\\{ENV_NAME}: {}",
                io::Error::from_raw_os_error(status as i32)
            )));
        }

        let units: Vec<u16> = data[..size as usize]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let len = units
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(units.len());
        Ok(Some(String::from_utf16_lossy(&units[..len])))
    }

    pub(super) fn write_value(value: Option<&str>) -> Result<()> {
        let key = open_key(KEY_SET_VALUE)?;
        let name = wide(ENV_NAME);
        let status = match value {
            Some(value) => {
                let units = wide(value);
                unsafe {
                    RegSetValueExW(
                        key,
                        name.as_ptr(),
                        0,
                        REG_SZ,
                        units.as_ptr().cast(),
                        (units.len() * 2) as u32,
                    )
                }
            }
            None => unsafe { RegDeleteValueW(key, name.as_ptr()) },
        };
        unsafe { RegCloseKey(key) };
        if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
            return Err(Error::Config(format!(
                "write HKCU\\{ENVIRONMENT_KEY}\\{ENV_NAME}: {}",
                io::Error::from_raw_os_error(status as i32)
            )));
        }
        broadcast_environment_change();
        Ok(())
    }

    fn broadcast_environment_change() {
        let name = wide(ENVIRONMENT_KEY);
        let mut result = 0usize;
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                name.as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                1000,
                &mut result,
            );
        }
    }
}
