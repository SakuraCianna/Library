use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("权限不足：{0}")]
    PermissionDenied(String),
    #[error("本地存储错误：{0}")]
    Storage(String),
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub message: String,
}

impl From<AppError> for ErrorResponse {
    fn from(value: AppError) -> Self {
        Self {
            message: value.to_string(),
        }
    }
}
