use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

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
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
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

        Ok(serde_json::json!({
            "isFile": ft.is_file(),
            "isDirectory": ft.is_dir(),
            "isSymlink": ft.is_symlink(),
            "size": meta.len(),
            "atimeMs": meta.accessed().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64,
            "mtimeMs": meta.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64,
            "ctimeMs": meta.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64,
            "birthtimeMs": meta.created().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64,
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
