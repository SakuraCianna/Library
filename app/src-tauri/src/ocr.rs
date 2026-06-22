use std::path::Path;

use crate::error::AppError;
use crate::models::OcrSidecarRequest;

pub fn build_ocr_request(file_path: &Path, model_dir: &Path, tier: &str) -> OcrSidecarRequest {
    OcrSidecarRequest {
        file_path: file_path.to_string_lossy().to_string(),
        model_dir: model_dir.to_string_lossy().to_string(),
        tier: tier.to_string(),
    }
}

pub fn validate_ocr_inputs(file_path: &Path, model_dir: &Path, tier: &str) -> Result<(), AppError> {
    if !file_path.is_file() {
        return Err(AppError::Filesystem("OCR 输入文件不存在".to_string()));
    }
    if !model_dir.is_dir() {
        return Err(AppError::Filesystem("OCR 模型目录不存在".to_string()));
    }
    let missing_models = required_ocr_models(tier)
        .into_iter()
        .filter(|model_name| !model_dir.join(model_name).is_dir())
        .collect::<Vec<_>>();
    if !missing_models.is_empty() {
        return Err(AppError::Filesystem(format!(
            "OCR 模型不完整，缺少 {}",
            missing_models.join("、")
        )));
    }

    Ok(())
}

fn required_ocr_models(tier: &str) -> [String; 2] {
    [
        format!("PP-OCRv6_{tier}_det"),
        format!("PP-OCRv6_{tier}_rec"),
    ]
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{build_ocr_request, validate_ocr_inputs};

    #[test]
    fn validates_existing_file_and_model_dir() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let input = temp_dir.path().join("scan.pdf");
        let model_dir = temp_dir.path().join("models");
        fs::write(&input, "pdf").expect("input");
        fs::create_dir(&model_dir).expect("model dir");
        fs::create_dir(model_dir.join("PP-OCRv6_medium_det")).expect("det model dir");
        fs::create_dir(model_dir.join("PP-OCRv6_medium_rec")).expect("rec model dir");

        validate_ocr_inputs(&input, &model_dir, "medium").expect("inputs valid");
        let request = build_ocr_request(&input, &model_dir, "medium");

        assert_eq!(request.tier, "medium");
        assert!(request.file_path.ends_with("scan.pdf"));
    }
}
