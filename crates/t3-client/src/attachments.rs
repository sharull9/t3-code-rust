//! Native file selection and upload for `thread.turn.start` attachments.
//!
//! The authenticated RPC only mints a short-lived upload URL. The file bytes
//! are then POSTed to that signed URL without an Authorization header.

use std::{path::PathBuf, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::json;
use url::Url;

use crate::{Connection, RpcError};

pub const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;
pub const MAX_ATTACHMENTS: usize = 100;
pub const MAX_TOTAL_IMAGE_BYTES: u64 = 80 * 1024 * 1024;
const MAX_UPLOAD_ATTEMPTS: usize = 3;

const SUPPORTED_IMAGES: &[&str] = &["image/gif", "image/jpeg", "image/png", "image/webp"];

/// A file selected in the native file chooser. It stays local after a failed
/// upload, so callers can keep the same value and retry without asking again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalAttachment {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub kind: AttachmentKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    Image,
    File,
}

/// Exact metadata shape accepted in `message.dispatch`'s `attachments`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadedAttachment {
    #[serde(rename = "type")]
    pub kind: AttachmentKind,
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: u64,
}

impl Serialize for AttachmentKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(match self {
            Self::Image => "image",
            Self::File => "file",
        })
    }
}

impl<'de> Deserialize<'de> for AttachmentKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match String::deserialize(deserializer)?.as_str() {
            "image" => Ok(Self::Image),
            "file" => Ok(Self::File),
            other => Err(serde::de::Error::custom(format!("unknown attachment type `{other}`"))),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AttachmentError {
    #[error("could not inspect `{path}`: {message}")]
    Inspect { path: String, message: String },
    #[error("invalid attachment: {0}")]
    Invalid(String),
    #[error(transparent)]
    Rpc(#[from] RpcError),
    #[error("upload request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error(transparent)]
    Url(#[from] url::ParseError),
    #[error("attachment upload failed ({status}): {detail}")]
    Upload { status: u16, detail: String },
    #[error("could not read `{path}`: {message}")]
    Read { path: String, message: String },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateUploadUrlResult {
    attachment_id: String,
    relative_url: String,
}

impl LocalAttachment {
    /// Build metadata from a selected path and reject files the server cannot
    /// accept before they enter the composer queue.
    pub fn from_path(path: impl Into<PathBuf>) -> Result<Self, AttachmentError> {
        let path = path.into();
        let metadata = std::fs::metadata(&path).map_err(|error| AttachmentError::Inspect {
            path: path.display().to_string(),
            message: error.to_string(),
        })?;
        if !metadata.is_file() {
            return Err(AttachmentError::Invalid("the selected path is not a file".into()));
        }
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|name| !name.trim().is_empty())
            .ok_or_else(|| {
                AttachmentError::Invalid("the filename is empty or invalid UTF-8".into())
            })?
            .to_owned();
        if name.len() > 255 {
            return Err(AttachmentError::Invalid("filename is longer than 255 bytes".into()));
        }
        let mime_type = mime_type_for_path(&path);
        let kind = if SUPPORTED_IMAGES.contains(&mime_type.as_str()) {
            AttachmentKind::Image
        } else {
            AttachmentKind::File
        };
        let size_bytes = metadata.len();
        validate_size(kind, size_bytes)?;

        Ok(Self { id: uuid::Uuid::new_v4().to_string(), path, name, mime_type, size_bytes, kind })
    }

    pub fn validate(&self) -> Result<(), AttachmentError> {
        if self.name.trim().is_empty() || self.name.len() > 255 {
            return Err(AttachmentError::Invalid("filename must contain 1–255 bytes".into()));
        }
        if self.mime_type.trim().is_empty() || self.mime_type.len() > 100 {
            return Err(AttachmentError::Invalid("MIME type must contain 1–100 bytes".into()));
        }
        if matches!(self.kind, AttachmentKind::Image)
            && !SUPPORTED_IMAGES.contains(&self.mime_type.to_ascii_lowercase().as_str())
        {
            return Err(AttachmentError::Invalid(format!(
                "unsupported image type `{}`; use GIF, JPEG, PNG, or WebP",
                self.mime_type
            )));
        }
        validate_size(self.kind, self.size_bytes)
    }

    fn rpc_input(&self) -> serde_json::Value {
        json!({
            "type": match self.kind { AttachmentKind::Image => "image", AttachmentKind::File => "file" },
            "name": self.name,
            "mimeType": self.mime_type,
            "sizeBytes": self.size_bytes,
        })
    }
}

/// Enforce turn-wide attachment limits in the same way as the server
/// contract. This can be used when adding a selected batch to the panel.
pub fn validate_selection(files: &[LocalAttachment]) -> Result<(), AttachmentError> {
    if files.len() > MAX_ATTACHMENTS {
        return Err(AttachmentError::Invalid(format!(
            "you can attach up to {MAX_ATTACHMENTS} files per message"
        )));
    }
    let image_bytes = files.iter().filter(|file| file.kind == AttachmentKind::Image).try_fold(
        0u64,
        |sum, file| {
            file.validate()?;
            sum.checked_add(file.size_bytes)
                .ok_or_else(|| AttachmentError::Invalid("total image size overflowed".into()))
        },
    )?;
    if image_bytes > MAX_TOTAL_IMAGE_BYTES {
        return Err(AttachmentError::Invalid("images can total up to 80 MiB per message".into()));
    }
    for file in files {
        file.validate()?;
    }
    Ok(())
}

/// Mint a signed URL over the already-authenticated connection, then upload
/// raw bytes. Transient transfer failures get two retries with the same signed
/// URL; any final error leaves `local` untouched for the caller to retry.
pub async fn upload_attachment(
    connection: &Connection,
    _http: &reqwest::Client,
    base_url: &Url,
    local: &LocalAttachment,
) -> Result<UploadedAttachment, AttachmentError> {
    local.validate()?;
    let path = local.path.clone();
    let bytes = tokio::task::spawn_blocking(move || std::fs::read(path))
        .await
        .map_err(|error| AttachmentError::Read {
            path: local.path.display().to_string(),
            message: error.to_string(),
        })?
        .map_err(|error| AttachmentError::Read {
            path: local.path.display().to_string(),
            message: error.to_string(),
        })?;
    if bytes.len() as u64 != local.size_bytes {
        return Err(AttachmentError::Invalid(format!(
            "`{}` changed after selection; select it again before uploading",
            local.name
        )));
    }

    let minted: CreateUploadUrlResult =
        connection.rpc().call("attachments.createUploadUrl", local.rpc_input()).await?;
    let url = resolve_upload_url(base_url, &minted.relative_url)?;
    // The upload URL itself is a bearer capability. Do not let an HTTP
    // redirect forward its signed path to another origin.
    let upload_http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(90))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut last_failure = None;
    for attempt in 0..MAX_UPLOAD_ATTEMPTS {
        let result = upload_http
            .post(url.clone())
            .header(reqwest::header::CONTENT_TYPE, &local.mime_type)
            .body(bytes.clone())
            .send()
            .await;
        match result {
            Ok(response) if response.status().is_success() => {
                return Ok(UploadedAttachment {
                    kind: local.kind,
                    id: minted.attachment_id,
                    name: local.name.clone(),
                    mime_type: local.mime_type.clone(),
                    size_bytes: local.size_bytes,
                });
            }
            Ok(response) => {
                let status = response.status();
                let detail = response.text().await.unwrap_or_default();
                let transient = status == reqwest::StatusCode::REQUEST_TIMEOUT
                    || status == reqwest::StatusCode::TOO_MANY_REQUESTS
                    || status.is_server_error();
                if !transient || attempt + 1 == MAX_UPLOAD_ATTEMPTS {
                    return Err(AttachmentError::Upload { status: status.as_u16(), detail });
                }
                last_failure = Some(AttachmentError::Upload { status: status.as_u16(), detail });
            }
            Err(error) => {
                let transient = error.is_timeout() || error.is_connect() || error.is_request();
                if !transient || attempt + 1 == MAX_UPLOAD_ATTEMPTS {
                    return Err(AttachmentError::Request(error));
                }
                last_failure = Some(AttachmentError::Request(error));
            }
        }
        tokio::time::sleep(Duration::from_millis(200 * (1 << attempt))).await;
    }
    Err(last_failure.unwrap_or_else(|| AttachmentError::Invalid("upload did not run".into())))
}

fn validate_size(kind: AttachmentKind, size_bytes: u64) -> Result<(), AttachmentError> {
    let limit = match kind {
        AttachmentKind::Image => MAX_IMAGE_BYTES,
        AttachmentKind::File => MAX_FILE_BYTES,
    };
    if size_bytes == 0 {
        return Err(AttachmentError::Invalid("empty files cannot be attached".into()));
    }
    if size_bytes > limit {
        return Err(AttachmentError::Invalid(format!(
            "file exceeds the {} MiB attachment limit",
            limit / (1024 * 1024)
        )));
    }
    Ok(())
}

fn resolve_upload_url(base_url: &Url, relative_url: &str) -> Result<Url, AttachmentError> {
    let url = base_url.join(relative_url)?;
    if url.origin() != base_url.origin() {
        return Err(AttachmentError::Invalid(
            "the server returned an upload URL for a different origin".into(),
        ));
    }
    Ok(url)
}

fn mime_type_for_path(path: &std::path::Path) -> String {
    let extension = path.extension().and_then(|value| value.to_str()).unwrap_or_default();
    let mime = match extension.to_ascii_lowercase().as_str() {
        "gif" => "image/gif",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "csv" => "text/csv",
        "json" => "application/json",
        "yaml" | "yml" => "application/yaml",
        "html" | "htm" => "text/html",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => "application/octet-stream",
    };
    mime.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uploaded_metadata_matches_thread_turn_contract() {
        let attachment = UploadedAttachment {
            kind: AttachmentKind::Image,
            id: "att_123".into(),
            name: "screen shot.png".into(),
            mime_type: "image/png".into(),
            size_bytes: 123,
        };
        assert_eq!(
            serde_json::to_value(attachment).unwrap(),
            json!({
                "type": "image",
                "id": "att_123",
                "name": "screen shot.png",
                "mimeType": "image/png",
                "sizeBytes": 123,
            })
        );
    }

    #[test]
    fn selection_limits_match_server_contract() {
        let file = |name: &str| LocalAttachment {
            id: name.into(),
            path: PathBuf::from(name),
            name: name.into(),
            mime_type: "image/png".into(),
            size_bytes: MAX_IMAGE_BYTES,
            kind: AttachmentKind::Image,
        };
        let images = vec![file("a.png"), file("b.png")];
        assert!(validate_selection(&images).is_ok());
        let too_many_bytes = vec![file("a.png"); 9];
        assert!(validate_selection(&too_many_bytes).is_err());
    }

    #[test]
    fn signed_upload_urls_must_stay_on_the_server_origin() {
        let base = Url::parse("https://t3.example:3773/").unwrap();
        let relative = resolve_upload_url(&base, "/api/attachments/upload/signed").unwrap();
        assert_eq!(relative.origin(), base.origin());
        assert!(resolve_upload_url(&base, "https://attacker.example/upload").is_err());
    }
}
