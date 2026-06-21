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
        lancedb::connect(self.path.to_string_lossy().as_ref())
            .execute()
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::LanceVectorStore;

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
}
