//! The user's choices, kept as strings in HKCU\Software\DS Buddy. Writing
//! there needs no administrator rights, and the installer's -Uninstall
//! removes the key.

use std::{
    io,
    ptr::{null, null_mut},
};

use windows_sys::Win32::{
    Foundation::ERROR_SUCCESS,
    System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegGetValueW,
        RegSetValueExW,
    },
};

const KEY: &str = r"Software\DS Buddy";

pub fn load(name: &str) -> Option<String> {
    let key = wide(KEY);
    let name = wide(name);
    let mut buffer = [0u16; 256];
    let mut bytes = size_of_val(&buffer) as u32;
    // SAFETY: the strings are NUL-terminated and `bytes` is the buffer's size,
    // which RegGetValueW never writes past.
    let status = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, null_mut(), buffer.as_mut_ptr().cast(), &mut bytes)
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    // The size includes the terminating NUL.
    let len = (bytes as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buffer[..len]))
}

pub fn save(name: &str, value: &str) -> io::Result<()> {
    let key = wide(KEY);
    let name = wide(name);
    let value = wide(value);
    let mut handle: HKEY = null_mut();
    // SAFETY: the strings are NUL-terminated, `value` is passed with its byte
    // length, and the key handle is closed before returning.
    unsafe {
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            0,
            null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            null(),
            &mut handle,
            null_mut(),
        );
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let status = RegSetValueExW(handle, name.as_ptr(), 0, REG_SZ, value.as_ptr().cast(), (value.len() * 2) as u32);
        RegCloseKey(handle);
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
    }
    Ok(())
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
