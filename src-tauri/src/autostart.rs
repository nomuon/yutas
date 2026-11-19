//! Start with Windows: a value under the current user's Run key.
//! No admin rights needed, and it shows up in Task Manager's startup list.

use std::ptr::{null, null_mut};

use windows_sys::Win32::System::Registry::*;

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
#[cfg(not(test))]
const VALUE: &str = "yutas";
/// Tests use their own value so they never touch the real setting.
#[cfg(test)]
const VALUE: &str = "yutas-selftest";

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes to the real registry (a throwaway value, removed again), so it
    /// only runs on request: cargo test -- --ignored
    #[test]
    #[ignore]
    fn run_key_round_trip() {
        set(false).unwrap(); // removing a missing value is fine
        assert!(!is_enabled());
        set(true).unwrap();
        assert!(is_enabled());
        set(false).unwrap();
        assert!(!is_enabled());
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn open(access: u32) -> Option<HKEY> {
    let mut key: HKEY = null_mut();
    let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, access, &mut key) };
    (status == 0).then_some(key)
}

pub fn is_enabled() -> bool {
    let Some(key) = open(KEY_QUERY_VALUE) else { return false };
    let status = unsafe { RegQueryValueExW(key, wide(VALUE).as_ptr(), null(), null_mut(), null_mut(), null_mut()) };
    unsafe { RegCloseKey(key) };
    status == 0
}

pub fn set(enabled: bool) -> Result<(), String> {
    let key = open(KEY_SET_VALUE).ok_or("スタートアップの設定を開けません")?;
    let status = if enabled {
        let exe = std::env::current_exe().map_err(|e| format!("実行ファイルの場所がわかりません: {e}"))?;
        let command = wide(&format!("\"{}\"", exe.display()));
        unsafe {
            RegSetValueExW(
                key,
                wide(VALUE).as_ptr(),
                0,
                REG_SZ,
                command.as_ptr() as *const u8,
                (command.len() * 2) as u32,
            )
        }
    } else {
        const ERROR_FILE_NOT_FOUND: u32 = 2;
        match unsafe { RegDeleteValueW(key, wide(VALUE).as_ptr()) } {
            ERROR_FILE_NOT_FOUND => 0,
            other => other,
        }
    };
    unsafe { RegCloseKey(key) };
    if status == 0 {
        Ok(())
    } else {
        Err(format!("スタートアップの設定を変更できません (エラー {status})"))
    }
}
