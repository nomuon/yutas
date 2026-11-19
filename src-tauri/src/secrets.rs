//! The Anthropic API key. It is kept in the OS credential store (Windows
//! Credential Manager), never in config.json. ANTHROPIC_API_KEY in the
//! environment is used when nothing is stored.

const SERVICE: &str = "yutas";
const ACCOUNT: &str = "anthropic-api-key";

fn entry_for(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, account).map_err(|e| format!("資格情報マネージャーを開けません: {e}"))
}

fn entry() -> Result<keyring::Entry, String> {
    entry_for(ACCOUNT)
}

fn stored() -> Option<String> {
    entry().ok()?.get_password().ok().filter(|k| !k.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Touches the real credential store (under a throwaway account), so it
    /// only runs on request: cargo test -- --ignored
    #[test]
    #[ignore]
    fn credential_store_round_trip() {
        let entry = entry_for("selftest").unwrap();
        entry.set_password("sk-test-123").unwrap();
        assert_eq!(entry_for("selftest").unwrap().get_password().unwrap(), "sk-test-123");
        entry.delete_credential().unwrap();
        assert!(matches!(entry_for("selftest").unwrap().get_password(), Err(keyring::Error::NoEntry)));
    }
}

fn from_env() -> Option<String> {
    std::env::var("ANTHROPIC_API_KEY").ok().filter(|k| !k.trim().is_empty())
}

pub fn api_key() -> Option<String> {
    stored().or_else(from_env)
}

/// Where the key in use comes from: "stored", "env" or "none".
pub fn source() -> &'static str {
    if stored().is_some() {
        "stored"
    } else if from_env().is_some() {
        "env"
    } else {
        "none"
    }
}

pub fn store(key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("APIキーを入力してください".into());
    }
    entry()?.set_password(key).map_err(|e| format!("APIキーを保存できません: {e}"))
}

pub fn delete() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("APIキーを削除できません: {e}")),
    }
}
