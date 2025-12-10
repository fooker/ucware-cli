use anyhow::Result;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;
use tracing::debug;

pub struct TokenStore {
    token: RwLock<String>,
    path: PathBuf,
}

impl TokenStore {
    pub async fn with_token(path: impl AsRef<Path>, token: String) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        tokio::fs::write(&path, token.as_bytes()).await?;

        Ok(Self {
            path,
            token: RwLock::new(token),
        })
    }

    pub async fn open(path: impl AsRef<Path>) -> Result<Option<Self>> {
        let path = path.as_ref().to_path_buf();

        let token = if tokio::fs::try_exists(&path).await? {
            debug!("Loading existing token from store");
            tokio::fs::read_to_string(&path).await?.trim().to_string()
        } else {
            return Ok(None);
        };

        Ok(Some(Self {
            path,
            token: RwLock::new(token),
        }))
    }

    pub async fn get(&self) -> impl Deref<Target = String> {
        self.token.read().await
    }

    pub async fn update(&self, next_token: String) -> Result<()> {
        let mut curr_token = self.token.write().await;
        if *curr_token == next_token {
            return Ok(());
        }

        *curr_token = next_token;

        tokio::fs::write(&self.path, curr_token.as_bytes()).await?;

        Ok(())
    }
}
