use crate::storage::StoreError;
use async_trait::async_trait;
use aws_sdk_s3::{
    Client, error::ProvideErrorMetadata, presigning::PresigningConfig, primitives::ByteStream,
};
use image::{ImageFormat, ImageReader, Limits, codecs::jpeg::JpegEncoder};
use serde::Serialize;
use std::{io::Cursor, time::Duration};
use uuid::Uuid;

pub const MAX_UPLOAD_BYTES: i64 = 8 * 1024 * 1024;
const CONTENT_TYPE: &str = "image/jpeg";

#[derive(Debug, Serialize)]
pub struct UploadTicket {
    pub id: Uuid,
    pub url: String,
}

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("The JPEG upload is missing or invalid")]
    Invalid,
    #[error(transparent)]
    Store(#[from] StoreError),
}

#[async_trait]
pub trait Media: Send + Sync {
    async fn upload(&self, truck: Uuid, bytes: i64) -> Result<UploadTicket, MediaError>;
    async fn publish(&self, truck: Uuid, id: Uuid) -> Result<String, MediaError>;
}

pub struct S3Media {
    client: Client,
    bucket: String,
    public_site: String,
}

impl S3Media {
    pub fn new(client: Client, bucket: String, public_site: String) -> Self {
        Self {
            client,
            bucket,
            public_site,
        }
    }
}

fn jpeg_without_metadata(bytes: &[u8]) -> Result<Vec<u8>, MediaError> {
    let mut decoder = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Jpeg);
    let mut limits = Limits::default();
    limits.max_image_width = Some(6500);
    limits.max_image_height = Some(6500);
    limits.max_alloc = Some(96 * 1024 * 1024);
    decoder.limits(limits);
    let preview = decoder
        .decode()
        .map_err(|_| MediaError::Invalid)?
        .thumbnail(1800, 1800);
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 85)
        .encode_image(&preview)
        .map_err(|_| MediaError::Invalid)?;
    Ok(jpeg)
}

#[async_trait]
impl Media for S3Media {
    async fn upload(&self, truck: Uuid, bytes: i64) -> Result<UploadTicket, MediaError> {
        if !(1..=MAX_UPLOAD_BYTES).contains(&bytes) {
            return Err(MediaError::Invalid);
        }
        let id = Uuid::new_v4();
        let config = PresigningConfig::expires_in(Duration::from_secs(300))
            .map_err(|_| StoreError::unavailable("menu_upload_expiry"))?;
        let put = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(format!("uploads/{truck}/{id}.jpg"))
            .content_type(CONTENT_TYPE)
            .content_length(bytes)
            .presigned(config)
            .await
            .map_err(|_| StoreError::unavailable("sign_menu_upload"))?;
        Ok(UploadTicket {
            id,
            url: put.uri().to_string(),
        })
    }

    async fn publish(&self, truck: Uuid, id: Uuid) -> Result<String, MediaError> {
        let staging_key = format!("uploads/{truck}/{id}.jpg");
        let public_key = format!("data/menus/{truck}/{id}.jpg");
        let source = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&staging_key)
            .send()
            .await
            .map_err(|error| {
                if error.as_service_error().is_some_and(|e| e.is_no_such_key()) {
                    MediaError::Invalid
                } else {
                    StoreError::unavailable("get_menu_upload").into()
                }
            })?;
        if !source
            .content_length()
            .is_some_and(|bytes| (1..=MAX_UPLOAD_BYTES).contains(&bytes))
        {
            return Err(MediaError::Invalid);
        }
        let source = source
            .body
            .collect()
            .await
            .map_err(|_| StoreError::unavailable("read_menu_upload"))?
            .into_bytes();
        let jpeg = tokio::task::spawn_blocking(move || jpeg_without_metadata(&source))
            .await
            .map_err(|_| StoreError::unavailable("encode_menu_jpeg"))??;
        let result = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(&public_key)
            .content_type(CONTENT_TYPE)
            .cache_control("public, max-age=86400")
            .body(ByteStream::from(jpeg))
            .if_none_match("*")
            .send()
            .await;
        match result {
            Ok(_) => {}
            Err(error)
                if error.as_service_error().and_then(|e| e.code())
                    == Some("PreconditionFailed") => {}
            Err(_) => return Err(StoreError::unavailable("publish_menu").into()),
        }
        Ok(format!(
            "{}/{public_key}",
            self.public_site.trim_end_matches('/')
        ))
    }
}
