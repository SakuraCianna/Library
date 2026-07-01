use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::{env, fs};

use crate::models::{DeepSeekRuntimeStatus, OcrRuntimeStatus, RuntimeStatus, VisionRuntimeStatus};

const DEFAULT_DEEPSEEK_MODEL: &str = "deepseek-v4-flash";
const DEFAULT_DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
const DEFAULT_OCR_TIER: &str = "medium";
const REQUIRED_OCR_MODEL_FILES: [&str; 3] =
    ["inference.json", "inference.pdiparams", "inference.yml"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeepSeekConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrConfig {
    pub tier: String,
    pub model_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisionConfig {
    pub model_dir: PathBuf,
}

pub fn runtime_status(app_data_dir: &Path) -> RuntimeStatus {
    let local_env = load_local_env();
    let api_key = config_value("DEEPSEEK_API_KEY", &local_env);
    let model = config_value("DEEPSEEK_MODEL", &local_env)
        .unwrap_or_else(|| DEFAULT_DEEPSEEK_MODEL.to_string());
    let base_url = config_value("DEEPSEEK_BASE_URL", &local_env)
        .unwrap_or_else(|| DEFAULT_DEEPSEEK_BASE_URL.to_string());
    let ocr = ocr_config_from_values(app_data_dir, &local_env);
    let vision = vision_config_from_values(app_data_dir, &local_env);

    build_runtime_status(
        api_key.as_deref(),
        model,
        base_url,
        ocr.tier,
        ocr.model_dir,
        vision.model_dir,
    )
}

pub fn deepseek_config() -> Option<DeepSeekConfig> {
    let local_env = load_local_env();
    let api_key = config_value("DEEPSEEK_API_KEY", &local_env)?;

    if api_key.trim().is_empty() {
        return None;
    }

    Some(DeepSeekConfig {
        api_key,
        model: config_value("DEEPSEEK_MODEL", &local_env)
            .unwrap_or_else(|| DEFAULT_DEEPSEEK_MODEL.to_string()),
        base_url: config_value("DEEPSEEK_BASE_URL", &local_env)
            .unwrap_or_else(|| DEFAULT_DEEPSEEK_BASE_URL.to_string()),
    })
}

pub fn ocr_config(app_data_dir: &Path) -> OcrConfig {
    let local_env = load_local_env();
    ocr_config_from_values(app_data_dir, &local_env)
}

fn ocr_config_from_values(app_data_dir: &Path, local_env: &HashMap<String, String>) -> OcrConfig {
    let tier = normalize_ocr_tier(config_value("OCR_MODEL_TIER", local_env).as_deref());
    let model_dir = config_value("OCR_MODEL_DIR", local_env)
        .map(PathBuf::from)
        .unwrap_or_else(|| default_ocr_model_dir(app_data_dir));

    OcrConfig { tier, model_dir }
}

pub fn vision_config(app_data_dir: &Path) -> VisionConfig {
    let local_env = load_local_env();
    vision_config_from_values(app_data_dir, &local_env)
}

fn vision_config_from_values(
    app_data_dir: &Path,
    local_env: &HashMap<String, String>,
) -> VisionConfig {
    let model_dir = config_value("VISION_MODEL_DIR", local_env)
        .map(PathBuf::from)
        .unwrap_or_else(|| default_vision_model_dir(app_data_dir));

    VisionConfig { model_dir }
}

fn build_runtime_status(
    api_key: Option<&str>,
    model: String,
    base_url: String,
    tier: String,
    ocr_model_dir: PathBuf,
    vision_model_dir: PathBuf,
) -> RuntimeStatus {
    let missing_ocr_models = missing_ocr_assets(&ocr_model_dir, &tier);
    let missing_vision_models = missing_vision_assets(&vision_model_dir);

    let configured = api_key
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);

    RuntimeStatus {
        deepseek: DeepSeekRuntimeStatus {
            configured,
            model,
            base_url,
            key_hint: api_key
                .map(redact_key)
                .unwrap_or_else(|| "未配置".to_string()),
        },
        ocr: OcrRuntimeStatus {
            configured: missing_ocr_models.is_empty(),
            tier,
            model_dir: ocr_model_dir.to_string_lossy().to_string(),
            missing_models: missing_ocr_models,
        },
        vision: VisionRuntimeStatus {
            configured: !missing_vision_models,
            model_dir: vision_model_dir.to_string_lossy().to_string(),
            missing_models: missing_vision_models,
        },
    }
}

fn missing_ocr_assets(model_dir: &Path, tier: &str) -> Vec<String> {
    [
        format!("PP-OCRv6_{tier}_det"),
        format!("PP-OCRv6_{tier}_rec"),
    ]
    .into_iter()
    .flat_map(|model_name| {
        let model_path = model_dir.join(&model_name);
        if !model_path.is_dir() {
            return vec![model_name];
        }

        REQUIRED_OCR_MODEL_FILES
            .iter()
            .filter_map(|file_name| {
                (!model_path.join(file_name).is_file()).then(|| format!("{model_name}/{file_name}"))
            })
            .collect::<Vec<_>>()
    })
    .collect()
}

fn default_ocr_model_dir(app_data_dir: &Path) -> PathBuf {
    if let Ok(current_dir) = env::current_dir() {
        for ancestor in current_dir.ancestors() {
            let candidate = ancestor.join("models").join("ocr").join("pp-ocrv6");
            if candidate.exists() {
                return candidate;
            }
        }
    }

    app_data_dir.join("models").join("ocr").join("pp-ocrv6")
}

fn missing_vision_assets(model_dir: &Path) -> bool {
    let required = ["config.json", "model.safetensors", "tokenizer.json"];
    for file in required {
        if !model_dir.join(file).is_file() {
            return true;
        }
    }
    false
}

fn default_vision_model_dir(app_data_dir: &Path) -> PathBuf {
    if let Ok(current_dir) = env::current_dir() {
        for ancestor in current_dir.ancestors() {
            let candidate = ancestor.join("models").join("vision").join("moondream2");
            if candidate.exists() {
                return candidate;
            }
        }
    }

    app_data_dir
        .join("models")
        .join("vision")
        .join("moondream2")
}

fn config_value(key: &str, local_env: &HashMap<String, String>) -> Option<String> {
    if let Ok(value) = env::var(key) {
        return (!value.trim().is_empty()).then_some(value);
    }

    local_env
        .get(key)
        .filter(|value| !value.trim().is_empty())
        .cloned()
}

fn load_local_env() -> HashMap<String, String> {
    discover_local_env_file()
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|content| parse_local_env_content(&content))
        .unwrap_or_default()
}

fn discover_local_env_file() -> Option<PathBuf> {
    let current_dir = env::current_dir().ok()?;

    for ancestor in current_dir.ancestors() {
        let candidate = ancestor.join(".env");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

fn parse_local_env_content(content: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }

        values.insert(key.to_string(), unquote_env_value(value.trim()).to_string());
    }

    values
}

fn unquote_env_value(value: &str) -> &str {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        let first = bytes[0];
        let last = bytes[value.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &value[1..value.len() - 1];
        }
    }

    value
}

fn normalize_ocr_tier(value: Option<&str>) -> String {
    match value.map(str::trim) {
        Some("tiny") => "tiny".to_string(),
        Some("small") => "small".to_string(),
        Some("medium") | Some("") | None => DEFAULT_OCR_TIER.to_string(),
        Some(_) => DEFAULT_OCR_TIER.to_string(),
    }
}

fn redact_key(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return "未配置".to_string();
    }

    let characters = trimmed.chars().collect::<Vec<_>>();
    if characters.len() <= 8 {
        return "已配置".to_string();
    }

    let prefix = characters.iter().take(3).collect::<String>();
    let suffix = characters
        .iter()
        .skip(characters.len().saturating_sub(4))
        .collect::<String>();
    format!("{prefix}...{suffix}")
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{build_runtime_status, normalize_ocr_tier, parse_local_env_content};

    #[test]
    fn redacts_deepseek_key_and_reports_missing_ocr_models() {
        let temp_dir = tempdir().expect("temp dir");
        let status = build_runtime_status(
            Some("test-key-12345678"),
            "deepseek-v4-flash".to_string(),
            "https://api.deepseek.com".to_string(),
            "medium".to_string(),
            temp_dir.path().to_path_buf(),
            temp_dir.path().to_path_buf(),
        );

        assert!(status.deepseek.configured);
        assert_eq!(status.deepseek.key_hint, "tes...5678");
        assert!(!status.ocr.configured);
        assert_eq!(status.ocr.missing_models.len(), 2);
    }

    #[test]
    fn redacts_non_ascii_key_without_panicking() {
        let temp_dir = tempdir().expect("temp dir");
        let status = build_runtime_status(
            Some("测试占位密钥很长文本"),
            "deepseek-v4-flash".to_string(),
            "https://api.deepseek.com".to_string(),
            "medium".to_string(),
            temp_dir.path().to_path_buf(),
            temp_dir.path().to_path_buf(),
        );

        assert!(status.deepseek.configured);
        assert_eq!(status.deepseek.key_hint, "测试占...很长文本");
    }

    #[test]
    fn empty_deepseek_key_is_not_reported_as_configured() {
        let temp_dir = tempdir().expect("temp dir");
        let status = build_runtime_status(
            Some("   "),
            "deepseek-v4-flash".to_string(),
            "https://api.deepseek.com".to_string(),
            "medium".to_string(),
            temp_dir.path().to_path_buf(),
            temp_dir.path().to_path_buf(),
        );

        assert!(!status.deepseek.configured);
        assert_eq!(status.deepseek.key_hint, "未配置");
    }

    #[test]
    fn detects_downloaded_ocr_model_folders() {
        let temp_dir = tempdir().expect("temp dir");
        std::fs::create_dir(temp_dir.path().join("PP-OCRv6_medium_det")).expect("det dir");
        std::fs::create_dir(temp_dir.path().join("PP-OCRv6_medium_rec")).expect("rec dir");
        for model_name in ["PP-OCRv6_medium_det", "PP-OCRv6_medium_rec"] {
            for file_name in ["inference.json", "inference.pdiparams", "inference.yml"] {
                std::fs::write(temp_dir.path().join(model_name).join(file_name), "model")
                    .expect("model file");
            }
        }

        let status = build_runtime_status(
            None,
            "deepseek-v4-flash".to_string(),
            "https://api.deepseek.com".to_string(),
            "medium".to_string(),
            temp_dir.path().to_path_buf(),
            temp_dir.path().to_path_buf(),
        );

        assert!(!status.deepseek.configured);
        assert_eq!(status.deepseek.key_hint, "未配置");
        assert!(status.ocr.configured);
        assert!(status.ocr.missing_models.is_empty());
    }

    #[test]
    fn falls_back_to_medium_for_invalid_ocr_tiers() {
        assert_eq!(normalize_ocr_tier(Some("tiny")), "tiny");
        assert_eq!(normalize_ocr_tier(Some("small")), "small");
        assert_eq!(normalize_ocr_tier(Some("medium")), "medium");
        assert_eq!(normalize_ocr_tier(Some("large")), "medium");
        assert_eq!(normalize_ocr_tier(None), "medium");
    }

    #[test]
    fn parses_local_env_without_including_comments() {
        let values = parse_local_env_content(
            "# DeepSeek API Key, 仅本机使用\nDEEPSEEK_API_KEY=local-test-key\nDEEPSEEK_MODEL=\"deepseek-v4-flash\"\n",
        );

        assert_eq!(
            values.get("DEEPSEEK_API_KEY").map(String::as_str),
            Some("local-test-key")
        );
        assert_eq!(
            values.get("DEEPSEEK_MODEL").map(String::as_str),
            Some("deepseek-v4-flash")
        );
        assert!(!values.contains_key("# DeepSeek API Key"));
    }
}
