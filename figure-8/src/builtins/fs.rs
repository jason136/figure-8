use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use tokio::fs::{
    OpenOptions, copy, create_dir_all, metadata, read_dir, read_to_string, remove_dir_all,
    remove_file, write,
};
use tokio::io::AsyncWriteExt;

use crate::sandbox::fn_def::FnDef;
use crate::sandbox::interface::Interface;
use crate::{FnDefError, JsApi, fn_def_async};

#[derive(Clone)]
pub struct Fs {
    backend: Arc<dyn FsBackend>,
}

impl Interface for Fs {
    fn extend_api(&self, js_api: &mut JsApi) -> Result<(), FnDefError> {
        js_api.extend_fn_defs(vec![
            read_file(self.backend.clone()),
            write_file(self.backend.clone()),
            append_file(self.backend.clone()),
            readdir(self.backend.clone()),
            mkdir(self.backend.clone()),
            rm(self.backend.clone()),
            raw_stat(self.backend.clone()),
            rename(self.backend.clone()),
            copy_file(self.backend.clone()),
            access(self.backend.clone()),
        ])?;

        js_api.push_polyfill(include_str!("polyfills/fs.js"));
        js_api.push_dts("fs", include_str!("polyfills/fs.d.ts"));

        Ok(())
    }
}

#[async_trait]
pub trait FsBackend: Send + Sync + 'static {
    async fn read_file(&self, path: &str) -> Result<String, FnDefError>;
    async fn write_file(&self, path: &str, data: &str) -> Result<(), FnDefError>;
    async fn append_file(&self, path: &str, data: &str) -> Result<(), FnDefError>;
    async fn readdir(&self, path: &str) -> Result<Vec<String>, FnDefError>;
    async fn mkdir(&self, path: &str) -> Result<(), FnDefError>;
    async fn rm(&self, path: &str) -> Result<(), FnDefError>;
    async fn stat(&self, path: &str) -> Result<serde_json::Value, FnDefError>;
    async fn rename(&self, old_path: &str, new_path: &str) -> Result<(), FnDefError>;
    async fn copy_file(&self, src: &str, dest: &str) -> Result<(), FnDefError>;
    async fn access(&self, path: &str) -> Result<(), FnDefError>;
}

impl Fs {
    pub fn from_local_path(path: PathBuf) -> Self {
        Fs {
            backend: Arc::new(LocalFsBackend { root: path }),
        }
    }

    pub fn from_s3(
        client: aws_sdk_s3::Client,
        bucket: impl Into<String>,
        prefix: impl Into<String>,
    ) -> Self {
        Fs {
            backend: Arc::new(S3FsBackend {
                client,
                bucket: bucket.into(),
                prefix: prefix.into().trim_end_matches('/').to_string(),
            }),
        }
    }
}

fn assert_safe_path(path: &str) -> Result<(), FnDefError> {
    for component in Path::new(path).components() {
        if let Component::ParentDir | Component::RootDir | Component::Prefix(_) = component {
            return Err(FnDefError::UnsafePath(path.to_string()));
        }
    }

    Ok(())
}

fn read_file(handle: Arc<dyn FsBackend>) -> FnDef {
    fn_def_async!(
        "fs.readFile",
        "Read a file and return its contents as a UTF-8 string",
        |path: String| -> String {
            let handle = handle.clone();
            async move {
                assert_safe_path(&path)?;
                handle.read_file(&path).await
            }
        }
    )
}

fn write_file(handle: Arc<dyn FsBackend>) -> FnDef {
    fn_def_async!(
        "fs.writeFile",
        "Write string data to a file, creating it and parent directories as needed",
        |file: String, data: String| -> () {
            let handle = handle.clone();
            async move {
                assert_safe_path(&file)?;
                handle.write_file(&file, &data).await
            }
        }
    )
}

fn append_file(handle: Arc<dyn FsBackend>) -> FnDef {
    fn_def_async!(
        "fs.appendFile",
        "Append string data to a file, creating it if it doesn't exist",
        |path: String, data: String| -> () {
            let handle = handle.clone();
            async move {
                assert_safe_path(&path)?;
                handle.append_file(&path, &data).await
            }
        }
    )
}

fn readdir(handle: Arc<dyn FsBackend>) -> FnDef {
    fn_def_async!(
        "fs.readdir",
        "Read the contents of a directory",
        |path: String| -> Vec<String> {
            let handle = handle.clone();
            async move {
                assert_safe_path(&path)?;
                handle.readdir(&path).await
            }
        }
    )
}

fn mkdir(handle: Arc<dyn FsBackend>) -> FnDef {
    fn_def_async!(
        "fs.mkdir",
        "Create a directory and any missing parent directories",
        |path: String| -> () {
            let handle = handle.clone();
            async move {
                assert_safe_path(&path)?;
                handle.mkdir(&path).await
            }
        }
    )
}

fn rm(handle: Arc<dyn FsBackend>) -> FnDef {
    fn_def_async!(
        "fs.rm",
        "Remove a file or directory recursively",
        |path: String| -> () {
            let handle = handle.clone();
            async move {
                assert_safe_path(&path)?;
                handle.rm(&path).await
            }
        }
    )
}

fn raw_stat(handle: Arc<dyn FsBackend>) -> FnDef {
    fn_def_async!(
        "fs.__rawStat",
        "Internal: get file metadata",
        |path: String| -> serde_json::Value {
            let handle = handle.clone();
            async move {
                assert_safe_path(&path)?;
                handle.stat(&path).await
            }
        }
    )
}

#[allow(non_snake_case)]
fn rename(handle: Arc<dyn FsBackend>) -> FnDef {
    fn_def_async!(
        "fs.rename",
        "Rename a file or directory",
        |oldPath: String, newPath: String| -> () {
            let handle = handle.clone();
            async move {
                assert_safe_path(&oldPath)?;
                assert_safe_path(&newPath)?;
                handle.rename(&oldPath, &newPath).await
            }
        }
    )
}

fn copy_file(handle: Arc<dyn FsBackend>) -> FnDef {
    fn_def_async!(
        "fs.copyFile",
        "Copy a file from src to dest",
        |src: String, dest: String| -> () {
            let handle = handle.clone();
            async move {
                assert_safe_path(&src)?;
                assert_safe_path(&dest)?;
                handle.copy_file(&src, &dest).await
            }
        }
    )
}

fn access(handle: Arc<dyn FsBackend>) -> FnDef {
    fn_def_async!(
        "fs.access",
        "Check accessibility of a path, throws if not accessible",
        |path: String| -> () {
            let handle = handle.clone();
            async move {
                assert_safe_path(&path)?;
                handle.access(&path).await
            }
        }
    )
}

#[derive(Clone)]
pub struct LocalFsBackend {
    root: PathBuf,
}

#[async_trait]
impl FsBackend for LocalFsBackend {
    async fn read_file(&self, path: &str) -> Result<String, FnDefError> {
        let path = self.root.join(path);

        Ok(read_to_string(&path).await?)
    }

    async fn write_file(&self, path: &str, data: &str) -> Result<(), FnDefError> {
        let path = self.root.join(path);

        if let Some(parent) = path.parent() {
            create_dir_all(parent).await?;
        }
        write(&path, data).await?;

        Ok(())
    }

    async fn append_file(&self, path: &str, data: &str) -> Result<(), FnDefError> {
        let path = self.root.join(path);

        if let Some(parent) = path.parent() {
            create_dir_all(parent).await?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;

        file.write_all(data.as_bytes()).await?;

        Ok(())
    }

    async fn readdir(&self, path: &str) -> Result<Vec<String>, FnDefError> {
        let path = self.root.join(path);
        let mut entries = read_dir(&path).await?;
        let mut names = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }

        names.sort();

        Ok(names)
    }

    async fn mkdir(&self, path: &str) -> Result<(), FnDefError> {
        let path = self.root.join(path);
        create_dir_all(&path).await?;

        Ok(())
    }

    async fn rm(&self, path: &str) -> Result<(), FnDefError> {
        let path = self.root.join(path);
        let meta = metadata(&path).await?;

        if meta.is_dir() {
            remove_dir_all(&path).await?;
        } else {
            remove_file(&path).await?;
        }

        Ok(())
    }

    async fn stat(&self, path: &str) -> Result<serde_json::Value, FnDefError> {
        let path = self.root.join(path);
        let meta = metadata(&path).await?;
        let ft = meta.file_type();

        let ts = |r: std::io::Result<SystemTime>| -> f64 {
            r.ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as f64)
                .unwrap_or(0.0)
        };

        Ok(serde_json::json!({
            "isFile": ft.is_file(),
            "isDirectory": ft.is_dir(),
            "size": meta.len(),
            "atimeMs": ts(meta.accessed()),
            "mtimeMs": ts(meta.modified()),
            "ctimeMs": ts(meta.modified()),
            "birthtimeMs": ts(meta.created()),
        }))
    }

    async fn rename(&self, old_path: &str, new_path: &str) -> Result<(), FnDefError> {
        let old = self.root.join(old_path);
        let new = self.root.join(new_path);
        tokio::fs::rename(&old, &new).await?;
        Ok(())
    }

    async fn copy_file(&self, src: &str, dest: &str) -> Result<(), FnDefError> {
        let src = self.root.join(src);
        let dest = self.root.join(dest);
        copy(&src, &dest).await?;

        Ok(())
    }

    async fn access(&self, path: &str) -> Result<(), FnDefError> {
        let path = self.root.join(path);
        metadata(&path).await?;

        Ok(())
    }
}

#[derive(Clone)]
pub struct S3FsBackend {
    client: aws_sdk_s3::Client,
    bucket: String,
    /// Key prefix within the bucket, stored without trailing slash.
    prefix: String,
}

impl S3FsBackend {
    fn normalize(path: &str) -> &str {
        let p = path.strip_prefix("./").unwrap_or(path);
        if p == "." || p.is_empty() { "" } else { p }
    }

    fn key(&self, path: &str) -> String {
        let path = Self::normalize(path);
        if self.prefix.is_empty() {
            path.to_string()
        } else if path.is_empty() {
            self.prefix.clone()
        } else {
            format!("{}/{}", self.prefix, path)
        }
    }

    fn dir_prefix(&self, path: &str) -> String {
        let key = self.key(path);
        if key.is_empty() {
            String::new()
        } else if key.ends_with('/') {
            key
        } else {
            format!("{key}/")
        }
    }
}

#[async_trait]
impl FsBackend for S3FsBackend {
    async fn read_file(&self, path: &str) -> Result<String, FnDefError> {
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(self.key(path))
            .send()
            .await
            .map_err(|e| FnDefError::Custom(format!("s3 GetObject: {e}")))?;

        let bytes = resp
            .body
            .collect()
            .await
            .map_err(|e| FnDefError::Custom(format!("s3 read body: {e}")))?;

        Ok(String::from_utf8_lossy(&bytes.into_bytes()).into_owned())
    }

    async fn write_file(&self, path: &str, data: &str) -> Result<(), FnDefError> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(self.key(path))
            .body(aws_sdk_s3::primitives::ByteStream::from(
                data.as_bytes().to_vec(),
            ))
            .send()
            .await
            .map_err(|e| FnDefError::Custom(format!("s3 PutObject: {e}")))?;

        Ok(())
    }

    async fn append_file(&self, path: &str, data: &str) -> Result<(), FnDefError> {
        let existing = match self.read_file(path).await {
            Ok(content) => content,
            Err(_) => String::new(),
        };
        self.write_file(path, &format!("{existing}{data}")).await
    }

    async fn readdir(&self, path: &str) -> Result<Vec<String>, FnDefError> {
        let prefix = self.dir_prefix(path);
        let mut names = Vec::new();
        let mut continuation_token: Option<String> = None;

        loop {
            let mut req = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(&prefix)
                .delimiter("/");

            if let Some(token) = continuation_token.take() {
                req = req.continuation_token(token);
            }

            let resp = req
                .send()
                .await
                .map_err(|e| FnDefError::Custom(format!("s3 ListObjectsV2: {e}")))?;

            for obj in resp.contents() {
                if let Some(name) = obj.key().and_then(|k| k.strip_prefix(&prefix)) {
                    if !name.is_empty() {
                        names.push(name.to_string());
                    }
                }
            }

            for cp in resp.common_prefixes() {
                if let Some(name) = cp.prefix().and_then(|p| p.strip_prefix(&prefix)) {
                    let name = name.trim_end_matches('/');
                    if !name.is_empty() {
                        names.push(name.to_string());
                    }
                }
            }

            if resp.is_truncated() == Some(true) {
                continuation_token = resp.next_continuation_token().map(String::from);
            } else {
                break;
            }
        }

        names.sort();
        Ok(names)
    }

    async fn mkdir(&self, _path: &str) -> Result<(), FnDefError> {
        Ok(())
    }

    async fn rm(&self, path: &str) -> Result<(), FnDefError> {
        let key = self.key(path);

        if self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .is_ok()
        {
            self.client
                .delete_object()
                .bucket(&self.bucket)
                .key(&key)
                .send()
                .await
                .map_err(|e| FnDefError::Custom(format!("s3 DeleteObject: {e}")))?;
            return Ok(());
        }

        let prefix = self.dir_prefix(path);
        let mut continuation_token: Option<String> = None;

        loop {
            let mut req = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(&prefix);

            if let Some(token) = continuation_token.take() {
                req = req.continuation_token(token);
            }

            let resp = req
                .send()
                .await
                .map_err(|e| FnDefError::Custom(format!("s3 ListObjectsV2: {e}")))?;

            let objects: Vec<_> = resp
                .contents()
                .iter()
                .filter_map(|obj| {
                    aws_sdk_s3::types::ObjectIdentifier::builder()
                        .key(obj.key()?)
                        .build()
                        .ok()
                })
                .collect();

            if !objects.is_empty() {
                self.client
                    .delete_objects()
                    .bucket(&self.bucket)
                    .delete(
                        aws_sdk_s3::types::Delete::builder()
                            .set_objects(Some(objects))
                            .build()
                            .map_err(|e| FnDefError::Custom(format!("s3 Delete build: {e}")))?,
                    )
                    .send()
                    .await
                    .map_err(|e| FnDefError::Custom(format!("s3 DeleteObjects: {e}")))?;
            }

            if resp.is_truncated() == Some(true) {
                continuation_token = resp.next_continuation_token().map(String::from);
            } else {
                break;
            }
        }

        Ok(())
    }

    async fn stat(&self, path: &str) -> Result<serde_json::Value, FnDefError> {
        let key = self.key(path);

        if let Ok(head) = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
        {
            let mtime_ms = head
                .last_modified()
                .map(|t| t.as_secs_f64() * 1000.0)
                .unwrap_or(0.0);

            return Ok(serde_json::json!({
                "isFile": true,
                "isDirectory": false,
                "size": head.content_length().unwrap_or(0),
                "atimeMs": mtime_ms,
                "mtimeMs": mtime_ms,
                "ctimeMs": mtime_ms,
                "birthtimeMs": mtime_ms,
            }));
        }

        let prefix = self.dir_prefix(path);
        let resp = self
            .client
            .list_objects_v2()
            .bucket(&self.bucket)
            .prefix(&prefix)
            .max_keys(1)
            .send()
            .await
            .map_err(|e| FnDefError::Custom(format!("s3 ListObjectsV2: {e}")))?;

        if resp.key_count().unwrap_or(0) > 0 {
            Ok(serde_json::json!({
                "isFile": false,
                "isDirectory": true,
                "size": 0,
                "atimeMs": 0.0,
                "mtimeMs": 0.0,
                "ctimeMs": 0.0,
                "birthtimeMs": 0.0,
            }))
        } else {
            Err(FnDefError::Custom(format!("not found: {path}")))
        }
    }

    async fn rename(&self, old_path: &str, new_path: &str) -> Result<(), FnDefError> {
        self.copy_file(old_path, new_path).await?;
        self.rm(old_path).await
    }

    async fn copy_file(&self, src: &str, dest: &str) -> Result<(), FnDefError> {
        let copy_source = format!("{}/{}", self.bucket, self.key(src));

        self.client
            .copy_object()
            .bucket(&self.bucket)
            .copy_source(&copy_source)
            .key(self.key(dest))
            .send()
            .await
            .map_err(|e| FnDefError::Custom(format!("s3 CopyObject: {e}")))?;

        Ok(())
    }

    async fn access(&self, path: &str) -> Result<(), FnDefError> {
        let key = self.key(path);

        if self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .is_ok()
        {
            return Ok(());
        }

        let prefix = self.dir_prefix(path);
        let resp = self
            .client
            .list_objects_v2()
            .bucket(&self.bucket)
            .prefix(&prefix)
            .max_keys(1)
            .send()
            .await
            .map_err(|e| FnDefError::Custom(format!("s3 ListObjectsV2: {e}")))?;

        if resp.key_count().unwrap_or(0) > 0 {
            Ok(())
        } else {
            Err(FnDefError::Custom(format!("not accessible: {path}")))
        }
    }
}
