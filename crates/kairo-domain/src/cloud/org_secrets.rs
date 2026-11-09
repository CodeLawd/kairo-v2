use super::{OrgSecretsPatch, OrgSecretsPayload, SecretsConfigured};
use crate::settings::{AppSettings, LyricsSettings, SettingsSecretClearKey, SttSettings};
use SettingsSecretClearKey::*;
use serde_json::{Map, Value};

pub const STT_SECRET_KEYS: &[SettingsSecretClearKey] =
    &[ApiKey, AnthropicApiKey, DeepseekApiKey, BibleApiKey];
pub const LYRICS_SECRET_KEYS: &[SettingsSecretClearKey] = &[BraveApiKey, GoogleTranslateApiKey];

pub fn apply_org_secrets_to_settings(settings: &mut AppSettings, secrets: &OrgSecretsPayload) {
    settings.stt.api_key.clone_from(&secrets.deepgram_api_key);
    settings
        .stt
        .anthropic_api_key
        .clone_from(&secrets.anthropic_api_key);
    settings
        .stt
        .deepseek_api_key
        .clone_from(&secrets.deepseek_api_key);
    settings
        .stt
        .bible_api_key
        .clone_from(&secrets.bible_api_key);
    settings
        .lyrics
        .brave_api_key
        .clone_from(&secrets.brave_api_key);
    settings
        .lyrics
        .google_translate_api_key
        .clone_from(&secrets.google_translate_api_key);
}

pub fn settings_to_org_secrets_patch(
    settings: &AppSettings,
    clear: &[SettingsSecretClearKey],
) -> OrgSecretsPatch {
    let write = |key, value: &str| {
        if clear.contains(&key) {
            return Some(None);
        }
        let trimmed = crate::js::trim(value);
        (!trimmed.is_empty()).then(|| Some(trimmed.into()))
    };
    OrgSecretsPatch {
        deepgram_api_key: write(ApiKey, &settings.stt.api_key),
        anthropic_api_key: write(AnthropicApiKey, &settings.stt.anthropic_api_key),
        deepseek_api_key: write(DeepseekApiKey, &settings.stt.deepseek_api_key),
        bible_api_key: write(BibleApiKey, &settings.stt.bible_api_key),
        brave_api_key: write(BraveApiKey, &settings.lyrics.brave_api_key),
        google_translate_api_key: write(
            GoogleTranslateApiKey,
            &settings.lyrics.google_translate_api_key,
        ),
    }
}

pub fn merge_secret_section(
    current: &Map<String, Value>,
    incoming: &Map<String, Value>,
    secret_keys: &[SettingsSecretClearKey],
) -> Map<String, Value> {
    let mut next = current.clone();
    next.extend(incoming.clone());
    next.remove("clearKeys");
    let clear = incoming.get("clearKeys").and_then(Value::as_array);
    for key in secret_keys {
        let key = key.as_str();
        if clear.is_some_and(|list| list.iter().any(|value| value.as_str() == Some(key))) {
            next.insert(key.into(), "".into());
        } else if let Some(value) = incoming
            .get(key)
            .and_then(Value::as_str)
            .map(crate::js::trim)
            .filter(|v| !v.is_empty())
        {
            next.insert(key.into(), value.into());
        } else if let Some(value) = current.get(key) {
            next.insert(key.into(), value.clone());
        } else {
            next.remove(key);
        }
    }
    next
}

pub fn secrets_configured_from_settings(
    stt: &SttSettings,
    lyrics: &LyricsSettings,
) -> SecretsConfigured {
    let present = |value: &str| !crate::js::trim(value).is_empty();
    SecretsConfigured {
        deepgram: present(&stt.api_key),
        anthropic: present(&stt.anthropic_api_key),
        deepseek: present(&stt.deepseek_api_key),
        bible: present(&stt.bible_api_key),
        brave: present(&lyrics.brave_api_key),
        google_translate: present(&lyrics.google_translate_api_key),
    }
}

pub fn redact_settings_secrets(mut settings: AppSettings) -> AppSettings {
    settings.stt.api_key.clear();
    settings.stt.anthropic_api_key.clear();
    settings.stt.deepseek_api_key.clear();
    settings.stt.bible_api_key.clear();
    settings.lyrics.brave_api_key.clear();
    settings.lyrics.google_translate_api_key.clear();
    settings
}
