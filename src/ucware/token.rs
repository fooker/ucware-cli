use anyhow::Result;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;
use tracing::info;

pub struct TokenStore {
    token: RwLock<String>,
    path: PathBuf,
}

impl TokenStore {
    pub async fn open(
        path: impl AsRef<Path>,
        init: impl FnOnce() -> Result<String>,
    ) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        let token: String;
        if !tokio::fs::try_exists(&path).await? {
            info!("Initialize with new token");
            token = init()?;
        } else {
            info!("Loading existing token from store");
            token = tokio::fs::read_to_string(&path).await?.trim().to_string();
        }

        tokio::fs::write(&path, token.as_bytes()).await?;

        return Ok(Self {
            path,
            token: RwLock::new(token),
        });
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
