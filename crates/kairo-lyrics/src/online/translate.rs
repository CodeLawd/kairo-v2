//! Bilingual glosses under lyric lines. From `src/main/services/lyrics/translate.ts`.
//!
//! Preference: English published on lyric sites, then the managed LLM
//! (Anthropic or DeepSeek), then Google Translate, all through the gateway.
//! Once a real lyric-site edition covers enough lines, the rest stay bare
//! rather than filled with machine guesses.

use super::glosses::GlossFinder;
use super::http::{GatewayError, ManagedGateway};
use kairo_domain::cloud::{HttpMethod, ManagedProvider, ManagedPurpose, ManagedRequest};
use kairo_domain::lyrics::LyricsSongSection;
use kairo_domain::lyrics::translate::{
    apply_line_glosses, collect_translatable_indices, is_likely_english_lyric, lookup_online_gloss,
    should_apply_gloss, strip_line_glosses,
};
use serde_json::{Value, json};
use std::collections::HashMap;

const BATCH_SIZE: usize = 100;
const LLM_BATCH_SIZE: usize = 40;
const MODEL_ANTHROPIC: &str = "claude-haiku-4-5";
const MODEL_DEEPSEEK: &str = "deepseek-flash";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LlmProvider {
    Anthropic,
    Deepseek,
}

#[derive(Clone, Debug, Default)]
pub struct TranslateOptions {
    pub target: Option<String>,
    /// BCP-47 / Google code, or `auto`.
    pub source_language: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    /// `None` uses Google Translate.
    pub llm: Option<LlmProvider>,
}

struct Translated {
    text: String,
    detected: Option<String>,
}

fn normalize_source(raw: Option<&str>) -> String {
    let code = kairo_domain::js::trim(raw.unwrap_or("auto")).to_lowercase();
    if code.is_empty() || code == "auto" || code == "detect" {
        "auto".into()
    } else {
        code
    }
}

fn llm_system(source: &str) -> String {
    let hint = if source == "auto" {
        "Auto-detect each line’s language (often Yoruba, Igbo, or mixed Nigerian worship)."
            .to_owned()
    } else {
        format!(
            "Source language code: {source}. Treat every line as that language unless it is clearly already English."
        )
    };
    format!(
        "You translate Christian / African gospel worship lyrics into clear English for a church operator cue sheet.
Rules:
- Return ONLY a JSON array of strings with the SAME length and order as the input.
- One English gloss per input line. No markdown, no commentary.
- Preserve proper names and titles (Amioluwa, Odudu, Jesus, Jehovah, Eje Jesu, etc.) when they function as names.
- Keep meaning faithful; do not invent new theology or rewrite the song.
- If a line is already English (or mostly English), return that line unchanged.
- {hint}"
    )
}

/// The JSON array inside an LLM reply, of exactly `expected` strings.
pub fn parse_json_string_array(raw: &str, expected: usize) -> Result<Vec<String>, String> {
    let trimmed = kairo_domain::js::trim(raw);
    let (Some(start), Some(end)) = (trimmed.find('['), trimmed.rfind(']')) else {
        return Err("LLM returned an unexpected translation format.".into());
    };
    if end < start {
        return Err("LLM returned an unexpected translation format.".into());
    }
    let parsed: Value = serde_json::from_str(&trimmed[start..=end])
        .map_err(|_| "LLM returned invalid JSON for translations.".to_owned())?;
    let Some(items) = parsed.as_array().filter(|a| a.len() == expected) else {
        return Err("LLM returned the wrong number of translated lines.".into());
    };
    Ok(items
        .iter()
        .map(|v| {
            if v.is_null() {
                String::new()
            } else {
                kairo_domain::js::value_to_string(v)
            }
        })
        .collect())
}

static KEY_REJECTED: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"(?i)API key|keyNotValid|invalid").unwrap());

fn google_error(error: &GatewayError) -> String {
    let message = &error.message;
    match error.status {
        Some(400) if KEY_REJECTED.is_match(message) => {
            "The Google Translate API key was rejected.".into()
        }
        Some(403) if message.contains("billing") => "Google Translate requires billing enabled on this Cloud project.".into(),
        Some(403) => "Google Translate access was denied. Check the API key and that Cloud Translation API is enabled.".into(),
        Some(429) => "Google Translate quota reached for now.".into(),
        _ if message.is_empty() => "Translation failed.".into(),
        _ => message.clone(),
    }
}

async fn translate_llm_chunk(
    gateway: &dyn ManagedGateway,
    provider: LlmProvider,
    texts: &[String],
    source: &str,
) -> Result<Vec<Translated>, String> {
    let system = llm_system(source);
    let user = serde_json::to_string(texts).unwrap_or_default();
    let request = match provider {
        LlmProvider::Anthropic => ManagedRequest {
            provider: ManagedProvider::Anthropic,
            path: "/v1/messages".into(),
            method: HttpMethod::Post,
            query: None,
            body: Some(json!({
                "model": MODEL_ANTHROPIC, "max_tokens": 4096, "temperature": 0.2,
                "system": system, "messages": [{ "role": "user", "content": user }],
            })),
            purpose: Some(ManagedPurpose::Translation),
        },
        LlmProvider::Deepseek => ManagedRequest {
            provider: ManagedProvider::Deepseek,
            path: "/chat/completions".into(),
            method: HttpMethod::Post,
            query: None,
            body: Some(json!({
                "model": MODEL_DEEPSEEK, "temperature": 0.2,
                "messages": [{ "role": "system", "content": system }, { "role": "user", "content": user }],
            })),
            purpose: Some(ManagedPurpose::Translation),
        },
    };
    let data = gateway.request(request).await.map_err(|e| {
        format!(
            "LLM translation failed. {}",
            if e.message.is_empty() {
                "Unknown error."
            } else {
                &e.message
            }
        )
    })?;
    let raw = match provider {
        LlmProvider::Anthropic => data["content"]
            .as_array()
            .and_then(|parts| parts.iter().find(|p| p["type"] == "text"))
            .and_then(|p| p["text"].as_str())
            .unwrap_or("")
            .to_owned(),
        LlmProvider::Deepseek => data["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_owned(),
    };
    let parsed = parse_json_string_array(&raw, texts.len())?;
    Ok(parsed
        .into_iter()
        .zip(texts)
        .map(|(text, original)| {
            let t = kairo_domain::js::trim(&text);
            let same = t.to_lowercase() == kairo_domain::js::trim(original).to_lowercase();
            Translated {
                text: if t.is_empty() {
                    original.clone()
                } else {
                    t.to_owned()
                },
                detected: Some(if same {
                    "en".into()
                } else if source == "auto" {
                    "und".into()
                } else {
                    source.to_owned()
                }),
            }
        })
        .collect())
}

async fn translate_google_chunk(
    gateway: &dyn ManagedGateway,
    texts: &[String],
    target: &str,
    source: &str,
) -> Result<Vec<Translated>, String> {
    let mut body = json!({ "q": texts, "target": target, "format": "text" });
    if source != "auto" {
        body["source"] = json!(source);
    }
    let request = ManagedRequest {
        provider: ManagedProvider::GoogleTranslate,
        path: "/language/translate/v2".into(),
        method: HttpMethod::Post,
        query: None,
        body: Some(body),
        purpose: Some(ManagedPurpose::Translation),
    };
    let data = gateway
        .request(request)
        .await
        .map_err(|e| google_error(&e))?;
    let Some(rows) = data["data"]["translations"]
        .as_array()
        .filter(|r| r.len() == texts.len())
    else {
        return Err("Unexpected response from Google Translate.".into());
    };
    Ok(rows
        .iter()
        .zip(texts)
        .map(|(row, original)| Translated {
            text: kairo_domain::js::trim(row["translatedText"].as_str().unwrap_or(original))
                .to_owned(),
            detected: row["detectedSourceLanguage"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| (source != "auto").then(|| source.to_owned())),
        })
        .collect())
}

/// Glosses under every translatable line; existing glosses are replaced.
pub async fn translate_sections(
    gateway: &dyn ManagedGateway,
    glosses: &GlossFinder,
    sections: &[LyricsSongSection],
    options: &TranslateOptions,
) -> Result<Vec<LyricsSongSection>, String> {
    let target = options
        .target
        .as_deref()
        .map(kairo_domain::js::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or("en")
        .to_owned();
    let source = normalize_source(options.source_language.as_deref());
    // Start from bare lyrics so "Translate again" replaces old glosses.
    let bare: Vec<LyricsSongSection> = sections
        .iter()
        .map(|s| LyricsSongSection {
            lines: strip_line_glosses(&s.lines),
            ..s.clone()
        })
        .collect();
    let mut jobs: Vec<(usize, usize, String)> = Vec::new();
    for (s, section) in bare.iter().enumerate() {
        for i in collect_translatable_indices(&section.lines) {
            jobs.push((s, i, section.lines[i].clone()));
        }
    }
    if jobs.is_empty() {
        return Ok(bare);
    }
    let online = match options
        .title
        .as_deref()
        .map(kairo_domain::js::trim)
        .filter(|t| !t.is_empty())
    {
        Some(title) => {
            let lines: Vec<String> = jobs.iter().map(|j| j.2.clone()).collect();
            glosses
                .fetch(title, options.artist.as_deref(), Some(&lines))
                .await
        }
        None => Vec::new(),
    };
    let mut maps: Vec<HashMap<usize, String>> = bare.iter().map(|_| HashMap::new()).collect();
    let mut pending: Vec<usize> = Vec::new();
    let (mut from_web, mut skipped_english, mut from_machine) = (0usize, 0usize, 0usize);
    for (index, (s, i, text)) in jobs.iter().enumerate() {
        if is_likely_english_lyric(text) {
            skipped_english += 1;
            continue;
        }
        if let Some(gloss) =
            lookup_online_gloss(text, &online).filter(|g| should_apply_gloss(text, g, Some("und")))
        {
            maps[*s].insert(*i, gloss);
            from_web += 1;
            continue;
        }
        pending.push(index);
    }
    // Machine output invents nonsense for Igbo/Yoruba/Igala worship: trust a
    // real edition once it covers a solid share.
    let coverage = from_web as f64 / (jobs.len() - skipped_english).max(1) as f64;
    let trust_web = from_web >= 2 && coverage >= 0.4;
    let mut engine = "web-only";
    if !trust_web && !pending.is_empty() {
        let texts: Vec<String> = pending.iter().map(|&j| jobs[j].2.clone()).collect();
        let mut translations = Vec::new();
        match options.llm {
            Some(provider) => {
                engine = if from_web > 0 { "web+llm" } else { "llm" };
                for chunk in texts.chunks(LLM_BATCH_SIZE) {
                    translations
                        .extend(translate_llm_chunk(gateway, provider, chunk, &source).await?);
                }
            }
            None => {
                engine = if from_web > 0 { "web+google" } else { "google" };
                for chunk in texts.chunks(BATCH_SIZE) {
                    translations
                        .extend(translate_google_chunk(gateway, chunk, &target, &source).await?);
                }
            }
        }
        for (p, &job) in pending.iter().enumerate() {
            let Some(result) = translations.get(p) else {
                continue;
            };
            let (s, i, text) = &jobs[job];
            let detected = if source != "auto" && engine.contains("google") {
                Some(source.clone())
            } else {
                result.detected.clone()
            };
            if !should_apply_gloss(text, &result.text, detected.as_deref()) {
                skipped_english += 1;
                continue;
            }
            maps[*s].insert(*i, result.text.clone());
            from_machine += 1;
        }
    }
    tracing::info!(
        engine,
        jobs = jobs.len(),
        from_web,
        from_machine,
        skipped_english,
        target,
        source,
        "lyrics translated"
    );
    Ok(bare
        .into_iter()
        .zip(maps)
        .map(|(section, map)| LyricsSongSection {
            lines: apply_line_glosses(&section.lines, &map),
            ..section
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_arrays_must_match_in_length_and_form() {
        assert_eq!(
            parse_json_string_array("Here: [\"a\", null, 3]", 3).unwrap(),
            ["a", "", "3"]
        );
        assert_eq!(
            parse_json_string_array("no array", 1).unwrap_err(),
            "LLM returned an unexpected translation format."
        );
        assert_eq!(
            parse_json_string_array("[1,", 1).unwrap_err(),
            "LLM returned an unexpected translation format."
        );
        assert_eq!(
            parse_json_string_array("[1, 2]", 1).unwrap_err(),
            "LLM returned the wrong number of translated lines."
        );
        assert_eq!(
            parse_json_string_array("[1, }]", 2).unwrap_err(),
            "LLM returned invalid JSON for translations."
        );
    }
}
