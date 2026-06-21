use std::path::{Path, PathBuf};

pub struct LanceVectorStore {
    path: PathBuf,
}

impl LanceVectorStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn connect(&self) -> lancedb::Result<lancedb::Connection> {
        lancedb::connect(self.local_uri()?).execute().await
    }

    fn local_uri(&self) -> lancedb::Result<&str> {
        let uri = self
            .path
            .to_str()
            .ok_or_else(|| lancedb::Error::InvalidInput {
                message: "本地向量库路径必须是有效的 Unicode 路径".to_string(),
            })?;

        if has_uri_scheme(uri) {
            return Err(lancedb::Error::InvalidInput {
                message: "本地向量库禁止使用远程 URI 或云端地址".to_string(),
            });
        }

        if !self.path.is_absolute() {
            return Err(lancedb::Error::InvalidInput {
                message: "本地向量库路径必须是绝对路径".to_string(),
            });
        }

        Ok(uri)
    }
}

fn has_uri_scheme(value: &str) -> bool {
    let Some((scheme, _)) = value.split_once(':') else {
        return false;
    };

    if scheme.len() == 1 && value.as_bytes().get(1) == Some(&b':') {
        return false;
    }

    !scheme.is_empty()
        && scheme.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
}

#[cfg(test)]
mod tests {
    use super::LanceVectorStore;
    use std::path::PathBuf;

    #[tokio::test]
    async fn connects_to_local_lancedb_path() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let store = LanceVectorStore::new(temp_dir.path().join("vectors.lance"));

        let connection = store
            .connect()
            .await
            .expect("local LanceDB connection opens");
        drop(connection);

        assert!(store.path().to_string_lossy().contains("vectors.lance"));
    }

    #[test]
    fn rejects_remote_lancedb_uri() {
        let store = LanceVectorStore::new(PathBuf::from("db://personal-knowledge"));
        let error = store.local_uri().expect_err("remote uri is rejected");

        assert!(error.to_string().contains("禁止使用远程 URI"));
    }

    #[test]
    fn rejects_cloud_object_store_uri() {
        let store = LanceVectorStore::new(PathBuf::from("s3://bucket/vectors"));
        let error = store.local_uri().expect_err("cloud uri is rejected");

        assert!(error.to_string().contains("禁止使用远程 URI"));
    }

    #[test]
    fn rejects_relative_db_like_path() {
        let store = LanceVectorStore::new(PathBuf::from("database/vectors.lance"));
        let error = store
            .local_uri()
            .expect_err("relative local path is rejected");

        assert!(error.to_string().contains("绝对路径"));
    }

    #[test]
    fn accepts_absolute_local_path_without_uri_scheme() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let store = LanceVectorStore::new(temp_dir.path().join("vectors.lance"));

        let uri = store.local_uri().expect("absolute local path is accepted");

        assert!(uri.contains("vectors.lance"));
        assert!(store.path().is_absolute());
    }
}
