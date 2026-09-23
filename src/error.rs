//! Per-file problems. Warnings do not affect the exit code; errors make it 2.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum FileError {
    #[error("暗号化または旧形式のため読めません")]
    EncryptedOrLegacy,
    #[error("未対応の形式です")]
    Unsupported,
    #[error("Excel ファイルの検索はまだ実装されていません")]
    ExcelNotYetSupported,
    #[error("ファイルが見つかりません")]
    NotFound,
    #[error("読み取れません: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip として読めません: {0}")]
    Zip(String),
    #[error("本文パートが見つかりません")]
    MissingMainPart,
    #[error("XML の解析に失敗しました ({part}): {message}")]
    Xml { part: String, message: String },
    #[error("検索中にエラーが発生しました: {0}")]
    Search(String),
}

impl FileError {
    /// Warnings are reported but do not count as errors.
    pub fn is_warning(&self) -> bool {
        matches!(
            self,
            FileError::EncryptedOrLegacy | FileError::Unsupported | FileError::ExcelNotYetSupported
        )
    }
}

impl From<zip::result::ZipError> for FileError {
    fn from(e: zip::result::ZipError) -> Self {
        match e {
            zip::result::ZipError::Io(io) => FileError::Io(io),
            other => FileError::Zip(other.to_string()),
        }
    }
}
