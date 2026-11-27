//! Update check against the GitHub releases. Installing is left to the
//! release's own install.ps1, which verifies the download's checksum, swaps
//! the executable while this one is closed and opens the new version.

use std::{
    env,
    ffi::c_void,
    io,
    path::Path,
    process::Command,
    ptr::{null, null_mut},
};

use windows_sys::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_DISABLE_REDIRECTS, WINHTTP_FLAG_SECURE,
    WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_QUERY_LOCATION, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryHeaders, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
};

const REPO: &str = "ndworks-dev/ds-buddy";
const TIMEOUT_MS: i32 = 10_000;

/// The latest release's version. github.com/<repo>/releases/latest redirects
/// to …/releases/tag/vX.Y.Z, so the Location header is enough: no JSON, and
/// none of the API's rate limit.
pub fn latest_version() -> io::Result<String> {
    let location = redirect_target("github.com", &format!("/{REPO}/releases/latest"))?;
    version_from_location(&location).ok_or_else(|| io::Error::other(format!("unexpected redirect to {location}")))
}

/// Whether `candidate` is a later release than `current`. Anything that isn't
/// a plain X.Y.Z, like a local "dev" build, is never offered an update.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((parse(candidate), parse(current)), (Some(a), Some(b)) if a > b)
}

/// Opens the install script for `version` in a PowerShell window, targeting
/// the folder this executable runs from. The caller must quit right after,
/// so the file can be replaced.
pub fn start(version: &str) -> io::Result<()> {
    let exe = env::current_exe()?;
    let dir = exe.parent().ok_or_else(|| io::Error::other("the executable has no folder"))?;
    // A copy run from the zip gets updated in place, without a Start menu
    // shortcut it never had.
    let installed = env::var_os("LOCALAPPDATA")
        .map(|d| Path::new(&d).join("Programs").join("DS Buddy"))
        .is_some_and(|d| d.to_string_lossy().eq_ignore_ascii_case(&dir.to_string_lossy()));
    Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &install_command(version, &dir.to_string_lossy(), installed)])
        .spawn()?;
    Ok(())
}

fn install_command(version: &str, dir: &str, shortcut: bool) -> String {
    let url = format!("https://github.com/{REPO}/releases/download/v{version}/install.ps1");
    let no_shortcut = if shortcut { "" } else { " -NoShortcut" };
    // On failure, keep the window open so the message can be read.
    format!(
        "[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12; \
         try {{ & ([scriptblock]::Create((irm {}))) -Version {} -InstallDir {}{no_shortcut} }} \
         catch {{ Write-Host $_ -ForegroundColor Red; Read-Host {} }}",
        ps_quote(&url),
        ps_quote(version),
        ps_quote(dir),
        ps_quote(crate::i18n::pick("Press Enter to close", "Pressione Enter para fechar")),
    )
}

/// A PowerShell single-quoted string, where only `'` needs escaping.
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.trim().trim_start_matches('v').split('.').map(|p| p.parse().ok());
    let v = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(v)
}

fn version_from_location(location: &str) -> Option<String> {
    let (_, tag) = location.trim().rsplit_once("/tag/v")?;
    parse(tag).map(|_| tag.to_string())
}

/// Sends a HEAD request over HTTPS without following redirects and returns
/// the Location header.
fn redirect_target(host: &str, path: &str) -> io::Result<String> {
    let agent = wide("ds-buddy-updater");
    let host = wide(host);
    let path = wide(path);
    let verb = wide("HEAD");
    // SAFETY: every pointer passed is either null where the API allows it or
    // points into a live, NUL-terminated buffer above; handles are closed by
    // `Handle` and never used after.
    unsafe {
        let session = Handle::new(WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, null(), null(), 0))?;
        WinHttpSetTimeouts(session.0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS);
        let connection = Handle::new(WinHttpConnect(session.0, host.as_ptr(), INTERNET_DEFAULT_HTTPS_PORT, 0))?;
        let request =
            Handle::new(WinHttpOpenRequest(connection.0, verb.as_ptr(), path.as_ptr(), null(), null(), null(), WINHTTP_FLAG_SECURE))?;
        let disable: u32 = WINHTTP_DISABLE_REDIRECTS;
        if WinHttpSetOption(request.0, WINHTTP_OPTION_DISABLE_FEATURE, (&raw const disable).cast(), 4) == 0
            || WinHttpSendRequest(request.0, null(), 0, null(), 0, 0, 0) == 0
            || WinHttpReceiveResponse(request.0, null_mut()) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = [0u16; 1024];
        let mut bytes = size_of_val(&buffer) as u32;
        if WinHttpQueryHeaders(request.0, WINHTTP_QUERY_LOCATION, null(), buffer.as_mut_ptr().cast(), &mut bytes, null_mut()) == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(String::from_utf16_lossy(&buffer[..bytes as usize / 2]))
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

struct Handle(*mut c_void);

impl Handle {
    fn new(handle: *mut c_void) -> io::Result<Self> {
        if handle.is_null() { Err(io::Error::last_os_error()) } else { Ok(Self(handle)) }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from WinHTTP and is closed exactly once.
        unsafe { WinHttpCloseHandle(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("0.4.0", "0.3.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0.0", "0.99.0"));
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("0.2.9", "0.3.0"));
        assert!(!is_newer("0.4.0", "dev"));
        assert!(!is_newer("0.4", "0.3.0"));
        assert!(!is_newer("0.4.0.1", "0.3.0"));
    }

    #[test]
    fn reads_the_release_redirect() {
        assert_eq!(version_from_location("https://github.com/ndworks-dev/ds-buddy/releases/tag/v0.3.0").as_deref(), Some("0.3.0"));
        assert_eq!(version_from_location("https://github.com/ndworks-dev/ds-buddy/releases"), None);
        assert_eq!(version_from_location("https://github.com/x/y/releases/tag/vnext"), None);
    }

    #[test]
    fn quotes_powershell_strings() {
        assert_eq!(ps_quote(r"C:\Users\O'Neil\DS Buddy"), r"'C:\Users\O''Neil\DS Buddy'");
        let command = install_command("0.4.0", r"C:\Apps\DS Buddy", false);
        assert!(command.contains("releases/download/v0.4.0/install.ps1'"));
        assert!(command.contains(r"-Version '0.4.0' -InstallDir 'C:\Apps\DS Buddy' -NoShortcut }"));
        assert!(!install_command("0.4.0", r"C:\x", true).contains("-NoShortcut"));
    }
}
