// Source tokens at rest. On Windows they are sealed for the signed-in user (DPAPI); on Linux they
// go to the desktop's keyring (Secret Service, through secret-tool): the config file alone does
// not give them away. Without a keyring the config file is readable by its owner only.
// Stored form: "dpapi:<hex>", "keyring:<source>" or "plain:<token>".

pub fn seal(source: &str, token: &str) -> String {
    #[cfg(windows)]
    if let Some(sealed) = windows::protect(token.as_bytes()) {
        let _ = source;
        return format!("dpapi:{}", hex(&sealed));
    }
    #[cfg(target_os = "linux")]
    if keyring::store(source, token) { return format!("keyring:{source}") }
    let _ = source;
    format!("plain:{token}")
}

pub fn open(stored: &str) -> Option<String> {
    if let Some(token) = stored.strip_prefix("plain:") { return Some(token.to_string()) }
    #[cfg(windows)]
    if let Some(data) = stored.strip_prefix("dpapi:") {
        return windows::unprotect(&unhex(data)?).and_then(|b| String::from_utf8(b).ok());
    }
    #[cfg(target_os = "linux")]
    if let Some(source) = stored.strip_prefix("keyring:") { return keyring::lookup(source) }
    None
}

/// A token no longer used leaves the keyring too.
pub fn forget(stored: &str) {
    #[cfg(target_os = "linux")]
    if let Some(source) = stored.strip_prefix("keyring:") { keyring::clear(source) }
    let _ = stored;
}

#[cfg(target_os = "linux")]
mod keyring {
    use std::io::Write;
    use std::process::{Command, Stdio};
    const ATTRS: [&str; 2] = ["application", "witchit-plus"];
    pub fn store(source: &str, token: &str) -> bool {
        let Ok(mut child) = Command::new("secret-tool")
            .args(["store", "--label", &format!("Witch It+ token for {source}"), ATTRS[0], ATTRS[1], "source", source])
            .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else { return false };
        let wrote = child.stdin.take().map(|mut i| i.write_all(token.as_bytes()).is_ok()).unwrap_or(false);
        child.wait().map(|s| s.success()).unwrap_or(false) && wrote
    }
    pub fn clear(source: &str) {
        let _ = Command::new("secret-tool").args(["clear", ATTRS[0], ATTRS[1], "source", source]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    pub fn lookup(source: &str) -> Option<String> {
        let out = Command::new("secret-tool").args(["lookup", ATTRS[0], ATTRS[1], "source", source]).stderr(Stdio::null()).output().ok()?;
        if !out.status.success() || out.stdout.is_empty() { return None }
        String::from_utf8(out.stdout).ok()
    }
}

/// Only the owner may read the file (not on Windows: the user profile is private already).
pub fn owner_only(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}

#[cfg_attr(not(windows), allow(dead_code))]
fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

#[cfg_attr(not(windows), allow(dead_code))]
fn unhex(text: &str) -> Option<Vec<u8>> {
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok()).collect()
}

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB};

    fn run(data: &[u8], protect: bool) -> Option<Vec<u8>> {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        let ok = unsafe {
            if protect {
                CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), 0, &mut output)
            } else {
                CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), 0, &mut output)
            }
        };
        if ok == 0 || output.pbData.is_null() { return None }
        let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
        unsafe { LocalFree(output.pbData as _) };
        Some(bytes)
    }
    pub fn protect(data: &[u8]) -> Option<Vec<u8>> { run(data, true) }
    pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> { run(data, false) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn round_trip() {
        assert_eq!(super::open("plain:github_pat_x").as_deref(), Some("github_pat_x"));
        assert_eq!(super::open("keyring:").as_deref().filter(|t| !t.is_empty()), None);
        assert_eq!(super::unhex(&super::hex(&[0, 15, 255])), Some(vec![0, 15, 255]));
    }
}
