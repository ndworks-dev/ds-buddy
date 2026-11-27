//! Thin safe wrappers over the classic Win32 Bluetooth API (bluetoothapis.dll / bthprops.cpl).

use std::{fmt, mem::size_of, ptr, str::FromStr};

use windows_sys::Win32::Devices::Bluetooth::{
    BLUETOOTH_ADDRESS, BLUETOOTH_DEVICE_INFO, BLUETOOTH_DEVICE_SEARCH_PARAMS, BLUETOOTH_FIND_RADIO_PARAMS, BLUETOOTH_SERVICE_ENABLE,
    BluetoothAuthenticateDeviceEx, BluetoothFindDeviceClose, BluetoothFindFirstDevice, BluetoothFindFirstRadio, BluetoothFindNextDevice,
    BluetoothFindRadioClose, BluetoothRemoveDevice, BluetoothSetServiceState, MITMProtectionNotRequired,
};
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_NO_MORE_ITEMS, GetLastError, HANDLE};
use windows_sys::core::GUID;

use crate::i18n;

/// Bluetooth HID service (0x1124), used by DualShock 4 and DualSense.
const HID_SERVICE: GUID = GUID::from_u128(0x00001124_0000_1000_8000_00805f9b34fb);

/// A 48-bit Bluetooth device address.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Address(pub u64);

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = self.0.to_be_bytes();
        write!(f, "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", b[2], b[3], b[4], b[5], b[6], b[7])
    }
}

impl FromStr for Address {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex: String = s.chars().filter(|c| !matches!(c, ':' | '-' | ' ')).collect();
        let invalid =
            || tr!("invalid address: '{s}' (use the form D0:BC:C1:9E:1D:4A)", "endereço inválido: '{s}' (use o formato D0:BC:C1:9E:1D:4A)");
        if hex.len() != 12 {
            return Err(invalid());
        }
        u64::from_str_radix(&hex, 16).map(Address).map_err(|_| invalid())
    }
}

/// A Bluetooth device as reported by Windows.
#[derive(Clone, Debug)]
pub struct Device {
    pub address: Address,
    pub name: String,
    pub class: u32,
    pub connected: bool,
    pub remembered: bool,
    pub authenticated: bool,
}

impl Device {
    fn from_info(info: &BLUETOOTH_DEVICE_INFO) -> Self {
        let len = info.szName.iter().position(|&c| c == 0).unwrap_or(info.szName.len());
        Device {
            // SAFETY: both union fields cover the same bytes; ullLong is always valid.
            address: Address(unsafe { info.Address.Anonymous.ullLong } & 0xFFFF_FFFF_FFFF),
            name: String::from_utf16_lossy(&info.szName[..len]),
            class: info.ulClassofDevice,
            connected: info.fConnected != 0,
            remembered: info.fRemembered != 0,
            authenticated: info.fAuthenticated != 0,
        }
    }

    /// Class of Device says "Peripheral / Gamepad".
    pub fn is_gamepad(&self) -> bool {
        let major = (self.class >> 8) & 0x1F;
        let minor = (self.class >> 2) & 0x0F;
        major == 0x05 && minor == 0x02
    }

    /// Looks like a DualShock 4 / DualSense (by name, or by gamepad class when the name is unknown).
    pub fn is_playstation_controller(&self) -> bool {
        let name = self.name.to_lowercase();
        name.contains("wireless controller")
            || name.contains("dualsense")
            || name.contains("dualshock")
            || (name.is_empty() && self.is_gamepad())
    }

    pub fn display_name(&self) -> &str {
        if self.name.is_empty() { i18n::pick("(unnamed)", "(sem nome)") } else { &self.name }
    }
}

/// Which devices a search should return.
#[derive(Clone, Copy, Default)]
pub struct Search {
    pub authenticated: bool,
    pub remembered: bool,
    pub unknown: bool,
    pub connected: bool,
    /// Run a radio inquiry for this many units of 1.28 s (1..=48). `None` = only cached devices.
    pub inquiry: Option<u8>,
}

/// Win32 error code with a readable description.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error(pub u32);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let os;
        let hint = match self.0 {
            31 => i18n::pick("the connection to the controller didn't complete", "a conexão com o controle não completou"),
            121 | 258 | 1460 => i18n::pick("the controller didn't respond in time", "o controle não respondeu a tempo"),
            1167 => i18n::pick(
                "the controller is out of range or left pairing mode",
                "o controle não está ao alcance ou saiu do modo de pareamento",
            ),
            1168 => i18n::pick("device not found", "dispositivo não encontrado"),
            1223 => i18n::pick(
                "pairing was canceled, or the Windows confirmation was declined",
                "o pareamento foi cancelado ou a confirmação do Windows foi recusada",
            ),
            1244 => i18n::pick("authentication failed", "a autenticação falhou"),
            // Windows already words its own message in the system language.
            _ => {
                os = std::io::Error::from_raw_os_error(self.0 as i32).to_string();
                &os
            }
        };
        let code = self.0;
        f.write_str(&tr!("code {code}: {hint}", "código {code}: {hint}"))
    }
}

impl std::error::Error for Error {}

/// Returns true if Windows has at least one Bluetooth radio.
pub fn radio_available() -> bool {
    let params = BLUETOOTH_FIND_RADIO_PARAMS { dwSize: size_of::<BLUETOOTH_FIND_RADIO_PARAMS>() as u32 };
    let mut radio: HANDLE = ptr::null_mut();
    // SAFETY: params and radio are valid for the duration of the call; handles are closed below.
    unsafe {
        let find = BluetoothFindFirstRadio(&params, &mut radio);
        if find.is_null() {
            return false;
        }
        CloseHandle(radio);
        BluetoothFindRadioClose(find);
    }
    true
}

/// Lists devices matching `search`. With `inquiry` set, this blocks while the radio scans.
pub fn find_devices(search: Search) -> Result<Vec<Device>, Error> {
    let params = BLUETOOTH_DEVICE_SEARCH_PARAMS {
        dwSize: size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32,
        fReturnAuthenticated: search.authenticated.into(),
        fReturnRemembered: search.remembered.into(),
        fReturnUnknown: search.unknown.into(),
        fReturnConnected: search.connected.into(),
        fIssueInquiry: search.inquiry.is_some().into(),
        cTimeoutMultiplier: search.inquiry.unwrap_or(0).clamp(0, 48),
        hRadio: ptr::null_mut(),
    };
    let mut info = new_info(Address(0));
    let mut devices = Vec::new();

    // SAFETY: params/info outlive the calls and info.dwSize is set; the find handle is closed.
    unsafe {
        let find = BluetoothFindFirstDevice(&params, &mut info);
        if find.is_null() {
            let err = GetLastError();
            return if err == ERROR_NO_MORE_ITEMS { Ok(devices) } else { Err(Error(err)) };
        }
        loop {
            devices.push(Device::from_info(&info));
            info = new_info(Address(0));
            if BluetoothFindNextDevice(find, &mut info) == 0 {
                break;
            }
        }
        BluetoothFindDeviceClose(find);
    }
    Ok(devices)
}

/// Pairs with the device at `address` (Just Works, no PIN), then enables its HID service.
/// Windows may show its own confirmation prompt while this blocks.
pub fn pair(address: Address) -> Result<(), Error> {
    let mut info = new_info(address);
    // SAFETY: info is a valid, initialized BLUETOOTH_DEVICE_INFO; null hwnd/radio/oob are allowed.
    let code =
        unsafe { BluetoothAuthenticateDeviceEx(ptr::null_mut(), ptr::null_mut(), &mut info, ptr::null(), MITMProtectionNotRequired) };
    if code != 0 {
        return Err(Error(code));
    }
    enable_hid(&info)
}

fn enable_hid(info: &BLUETOOTH_DEVICE_INFO) -> Result<(), Error> {
    // SAFETY: info and HID_SERVICE are valid for the call; null radio means "any radio".
    let code = unsafe { BluetoothSetServiceState(ptr::null_mut(), info, &HID_SERVICE, BLUETOOTH_SERVICE_ENABLE) };
    if code == 0 { Ok(()) } else { Err(Error(code)) }
}

/// Makes Windows forget the device. Returns `Ok(false)` if it was not known.
pub fn remove(address: Address) -> Result<bool, Error> {
    let addr = to_bt_address(address);
    // SAFETY: addr is a valid BLUETOOTH_ADDRESS for the duration of the call.
    match unsafe { BluetoothRemoveDevice(&addr) } {
        0 => Ok(true),
        1168 => Ok(false),
        code => Err(Error(code)),
    }
}

fn to_bt_address(address: Address) -> BLUETOOTH_ADDRESS {
    let mut addr = BLUETOOTH_ADDRESS::default();
    addr.Anonymous.ullLong = address.0;
    addr
}

fn new_info(address: Address) -> BLUETOOTH_DEVICE_INFO {
    BLUETOOTH_DEVICE_INFO { dwSize: size_of::<BLUETOOTH_DEVICE_INFO>() as u32, Address: to_bt_address(address), ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_round_trip() {
        let a: Address = "d0:bc:c1:9e:1d:4a".parse().unwrap();
        assert_eq!(a.0, 0xD0BC_C19E_1D4A);
        assert_eq!(a.to_string(), "D0:BC:C1:9E:1D:4A");
        assert_eq!("D0-BC-C1-9E-1D-4A".parse::<Address>().unwrap(), a);
        assert_eq!("D0BCC19E1D4A".parse::<Address>().unwrap(), a);
        assert!("D0:BC".parse::<Address>().is_err());
        assert!("ZZ:BC:C1:9E:1D:4A".parse::<Address>().is_err());
    }

    fn device(name: &str, class: u32) -> Device {
        Device { address: Address(0), name: name.into(), class, connected: false, remembered: false, authenticated: false }
    }

    #[test]
    fn detects_controllers() {
        assert!(device("DualSense Wireless Controller", 0).is_playstation_controller());
        assert!(device("Wireless Controller", 0).is_playstation_controller());
        assert!(device("", 0x002508).is_playstation_controller());
        assert!(!device("iPhone", 0x7A020C).is_playstation_controller());
        assert!(!device("", 0x240404).is_playstation_controller());
    }
}
