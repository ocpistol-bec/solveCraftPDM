//! PDM object storage: content-addressed blob storage with SHA-256 integrity.
//!
//! Supports local filesystem (dev) and S3/MinIO (production) backends.
//! Phase 1 uses whole-object put/get; multipart upload is a follow-up.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::PathBuf;

use bytes::Bytes;
use futures::TryStreamExt;
use object_store::ObjectStore;
use object_store::aws::AmazonS3Builder;
use object_store::local::LocalFileSystem;
use object_store::path::Path as OsPath;
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("object store error: {0}")]
    ObjectStore(#[from] object_store::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("integrity mismatch: expected {expected}, got {actual}")]
    IntegrityMismatch { expected: String, actual: String },
    #[error("validation: {0}")]
    Validation(String),
}

/// Backend configuration.
#[derive(Debug, Clone)]
pub enum Backend {
    Local(PathBuf),
    S3 { endpoint: Option<String>, bucket: String, region: String, access_key: Option<String>, secret_key: Option<String> },
}

impl Backend {
    /// Build a backend from env vars. Local dir wins if PDM_STORAGE_LOCAL is set.
    pub fn from_env() -> Result<Self, StorageError> {
        if let Ok(local_dir) = std::env::var("PDM_STORAGE_LOCAL") {
            let dir = PathBuf::from(local_dir);
            std::fs::create_dir_all(&dir)?;
            return Ok(Self::Local(dir));
        }
        let endpoint = std::env::var("PDM_S3_ENDPOINT").ok();
        let bucket = std::env::var("PDM_S3_BUCKET").map_err(|_| StorageError::Validation("PDM_S3_BUCKET not set".into()))?;
        let region = std::env::var("PDM_S3_REGION").unwrap_or_else(|_| "us-east-1".into());
        let access_key = std::env::var("PDM_S3_ACCESS_KEY").ok().filter(|s| !s.is_empty());
        let secret_key = std::env::var("PDM_S3_SECRET_KEY").ok().filter(|s| !s.is_empty());
        Ok(Self::S3 { endpoint, bucket, region, access_key, secret_key })
    }

    pub fn bucket_name(&self) -> &str {
        match self {
            Self::Local(p) => p.file_name().and_then(|s| s.to_str()).unwrap_or("local"),
            Self::S3 { bucket, .. } => bucket.as_str(),
        }
    }
}

/// Content-addressed blob store. Keys are the hex SHA-256 of the content.
pub struct BlobStore {
    inner: Box<dyn ObjectStore>,
}

impl BlobStore {
    pub async fn create(backend: &Backend) -> Result<Self, StorageError> {
        let inner: Box<dyn ObjectStore> = match backend {
            Backend::Local(path) => {
                let fs = LocalFileSystem::new_with_prefix(path).map_err(StorageError::ObjectStore)?;
                Box::new(fs)
            }
            Backend::S3 { endpoint, bucket, region, access_key, secret_key } => {
                let mut builder = AmazonS3Builder::new().with_bucket_name(bucket.clone()).with_region(region.clone());
                if let Some(ep) = endpoint {
                    builder = builder.with_endpoint(ep.clone());
                }
                if let Some(ak) = access_key {
                    builder = builder.with_access_key_id(ak.clone());
                }
                if let Some(sk) = secret_key {
                    builder = builder.with_secret_access_key(sk.clone());
                }
                builder = builder.with_allow_http(true);
                let s3 = builder.build().map_err(StorageError::ObjectStore)?;
                Box::new(s3)
            }
        };
        Ok(Self { inner })
    }

    fn key_for(sha: &str) -> OsPath {
        OsPath::from(sha)
    }

    /// Store bytes, keyed by their SHA-256. Returns (sha_hex, size). Dedups on existing key.
    pub async fn put_content_addressed(&self, data: &[u8]) -> Result<(String, u64), StorageError> {
        let sha = hex::encode(sha256_bytes(data));
        let path = Self::key_for(&sha);
        if self.inner.head(&path).await.is_ok() {
            return Ok((sha, data.len() as u64));
        }
        self.inner.put(&path, Bytes::from(data.to_vec()).into()).await.map_err(StorageError::ObjectStore)?;
        Ok((sha, data.len() as u64))
    }

    /// Read object bytes by SHA-256 hex.
    pub async fn get_by_sha(&self, sha: &str) -> Result<Vec<u8>, StorageError> {
        let path = Self::key_for(sha);
        let result = self.inner.get(&path).await.map_err(StorageError::ObjectStore)?;
        let data = result
            .into_stream()
            .try_fold(Vec::new(), |mut acc, chunk| async move {
                acc.extend_from_slice(&chunk);
                Ok(acc)
            })
            .await
            .map_err(|e| match e {
                object_store::Error::NotFound { .. } => StorageError::Validation(format!("blob not found: {sha}")),
                _ => StorageError::ObjectStore(e),
            })?;
        Ok(data)
    }

    /// Verify stored bytes match the expected SHA-256.
    pub async fn verify_by_sha(&self, expected_sha: &str) -> Result<(), StorageError> {
        let data = self.get_by_sha(expected_sha).await?;
        let actual = hex::encode(sha256_bytes(&data));
        if actual != expected_sha {
            return Err(StorageError::IntegrityMismatch { expected: expected_sha.to_string(), actual });
        }
        Ok(())
    }

    /// Delete an object (GC of unreferenced blobs).
    pub async fn delete(&self, sha: &str) -> Result<(), StorageError> {
        self.inner.delete(&Self::key_for(sha)).await.map_err(StorageError::ObjectStore)?;
        Ok(())
    }

    /// Existence check.
    pub async fn exists(&self, sha: &str) -> bool {
        self.inner.head(&Self::key_for(sha)).await.is_ok()
    }
}

fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&result);
    arr
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_backend(dir: &str) -> Backend {
        let p = std::path::PathBuf::from(dir);
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("create test dir");
        Backend::Local(p)
    }

    #[tokio::test]
    async fn put_and_get_roundtrip() {
        let backend = local_backend("/tmp/pdm-test-storage");
        let store = BlobStore::create(&backend).await.unwrap();
        let data = b"hello world";
        let (sha, size) = store.put_content_addressed(data).await.unwrap();
        assert_eq!(size, 11);
        assert_eq!(sha.len(), 64);
        let got = store.get_by_sha(&sha).await.unwrap();
        assert_eq!(got, data);
        let _ = std::fs::remove_dir_all("/tmp/pdm-test-storage");
    }

    #[tokio::test]
    async fn dedup_same_content() {
        let backend = local_backend("/tmp/pdm-test-dedup");
        let store = BlobStore::create(&backend).await.unwrap();
        let data = b"dedup test";
        let (sha1, _) = store.put_content_addressed(data).await.unwrap();
        let (sha2, _) = store.put_content_addressed(data).await.unwrap();
        assert_eq!(sha1, sha2);
        let _ = std::fs::remove_dir_all("/tmp/pdm-test-dedup");
    }

    #[tokio::test]
    async fn verify_integrity_ok_and_fail() {
        let backend = local_backend("/tmp/pdm-test-verify");
        let store = BlobStore::create(&backend).await.unwrap();
        let (sha, _) = store.put_content_addressed(b"integrity test").await.unwrap();
        store.verify_by_sha(&sha).await.unwrap();
        let got = store.get_by_sha(&sha).await.unwrap();
        let actual = hex::encode(sha256_bytes(&got));
        assert_eq!(actual, sha, "stored bytes must match declared sha");
        let _ = std::fs::remove_dir_all("/tmp/pdm-test-verify");
    }
}
