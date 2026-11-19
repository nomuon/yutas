//! User settings, stored as JSON in %APPDATA%\TypeBoost\config.json.
//! Read on every operation, so a save takes effect at once. The API key is
//! deliberately not kept here: it stays in the environment.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const DEFAULT_MODEL: &str = "claude-haiku-5-5";
pub const DEFAULT_TONE: &str =
    "です・ます調の、簡潔で丁寧なビジネスチャットの文体。文意は変えず、語尾や言い回しだけを整える。";

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct DictEntry {
    /// What the user types: a reading in kana, or a shorthand.
    pub reading: String,
    /// What it must become.
    pub word: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct AppRule {
    /// Executable name, e.g. "slack.exe".
    pub app: String,
    pub tone: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub trigger: String,
    pub model: String,
    pub workspace_id: String,
    pub tone: String,
    pub extra_instructions: String,
    pub dictionary: Vec<DictEntry>,
    pub app_rules: Vec<AppRule>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            trigger: std::env::var("TYPEBOOST_TRIGGER").unwrap_or_else(|_| "shift".into()),
            model: std::env::var("TYPEBOOST_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.into()),
            workspace_id: String::new(),
            tone: std::env::var("TYPEBOOST_TONE").unwrap_or_else(|_| DEFAULT_TONE.into()),
            extra_instructions: String::new(),
            dictionary: Vec::new(),
            app_rules: Vec::new(),
        }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        base.join("TypeBoost").join("config.json")
    }

    pub fn load() -> Settings {
        let Ok(text) = std::fs::read_to_string(Self::path()) else {
            return Settings::default();
        };
        match serde_json::from_str::<Settings>(&text) {
            Ok(settings) => settings.cleaned(),
            Err(e) => {
                crate::log(&format!("config.json is invalid, using defaults: {e}"));
                Settings::default()
            }
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("設定フォルダを作成できません: {e}"))?;
        }
        let json = serde_json::to_string_pretty(&self.clone().cleaned()).map_err(|e| e.to_string())?;
        std::fs::write(&path, json).map_err(|e| format!("設定を保存できません: {e}"))
    }

    /// Trim, drop empty rows, and fall back to defaults for blank essentials.
    fn cleaned(mut self) -> Settings {
        self.model = self.model.trim().to_string();
        if self.model.is_empty() {
            self.model = DEFAULT_MODEL.into();
        }
        if self.tone.trim().is_empty() {
            self.tone = DEFAULT_TONE.into();
        }
        self.workspace_id = self.workspace_id.trim().to_string();
        for entry in &mut self.dictionary {
            entry.reading = entry.reading.trim().to_string();
            entry.word = entry.word.trim().to_string();
        }
        self.dictionary.retain(|e| !e.reading.is_empty() && !e.word.is_empty());
        for rule in &mut self.app_rules {
            rule.app = rule.app.trim().to_lowercase();
        }
        self.app_rules.retain(|r| !r.app.is_empty() && !r.tone.trim().is_empty());
        self
    }

    /// The tone for the given executable, and the rule's app name if one matched.
    pub fn tone_for(&self, exe: Option<&str>) -> (String, Option<String>) {
        if let Some(exe) = exe {
            let exe = exe.to_lowercase();
            if let Some(rule) = self.app_rules.iter().find(|r| r.app.to_lowercase() == exe) {
                return (rule.tone.clone(), Some(rule.app.clone()));
            }
        }
        (self.tone.clone(), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_rule_overrides_default_tone() {
        let settings = Settings {
            tone: "default".into(),
            app_rules: vec![AppRule { app: "slack.exe".into(), tone: "casual".into() }],
            ..Settings::default()
        };
        assert_eq!(settings.tone_for(Some("Slack.exe")), ("casual".into(), Some("slack.exe".into())));
        assert_eq!(settings.tone_for(Some("notepad.exe")), ("default".into(), None));
        assert_eq!(settings.tone_for(None), ("default".into(), None));
    }

    #[test]
    fn partial_json_and_empty_rows() {
        let json = r#"{ "tone": " ", "dictionary": [ {"reading": "mtg", "word": "ミーティング"}, {"reading": "", "word": "x"} ] }"#;
        let settings = serde_json::from_str::<Settings>(json).unwrap().cleaned();
        assert_eq!(settings.tone, DEFAULT_TONE);
        assert_eq!(settings.dictionary.len(), 1);
        assert_eq!(settings.model, DEFAULT_MODEL);
    }
}
