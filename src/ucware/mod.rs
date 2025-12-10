use crate::sipsocket;
use crate::sipsocket::ServerTransaction;
pub use crate::ucware::token::TokenStore;
use crate::ucware::user::UserNamespaceClient;
use anyhow::{Context, Result};
use http::header::AUTHORIZATION;
use http::HeaderMap;
use jsonrpsee::core::client::ClientT;
use jsonrpsee::core::traits::ToRpcParams;
use jsonrpsee::http_client::HttpClient;
use serde::de::DeserializeOwned;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::sync::mpsc;
use url::Url;

mod token;
mod user;

trait Derive<T> {
    fn derive(&self) -> T;
}

pub trait Namespace {
    const PATH: &'static str;
}

pub struct NamespaceClient<Namespace: self::Namespace> {
    inner: Arc<Inner>,
    namespace: PhantomData<Namespace>,
}

impl<Namespace> Derive<NamespaceClient<Namespace>> for Client
where
    Namespace: self::Namespace,
{
    fn derive(&self) -> NamespaceClient<Namespace> {
        NamespaceClient {
            inner: self.inner.clone(),
            namespace: Default::default(),
        }
    }
}

pub trait Interface {
    const PATH: &'static str;
}

pub struct InterfaceClient<Namespace: self::Namespace, Interface: self::Interface> {
    inner: Arc<Inner>,
    namespace: PhantomData<Namespace>,
    interface: PhantomData<Interface>,
}

impl<Namespace, Interface> Derive<InterfaceClient<Namespace, Interface>>
    for NamespaceClient<Namespace>
where
    Namespace: self::Namespace,
    Interface: self::Interface,
{
    fn derive(&self) -> InterfaceClient<Namespace, Interface> {
        InterfaceClient {
            inner: self.inner.clone(),
            namespace: self.namespace,
            interface: Default::default(),
        }
    }
}

impl<Namespace, Interface> InterfaceClient<Namespace, Interface>
where
    Namespace: self::Namespace,
    Interface: self::Interface,
{
    async fn client(&self) -> Result<HttpClient> {
        let Inner {
            ref base_url,
            ref token,
        } = *self.inner;

        let url = base_url
            .join(&format!("{}/", Namespace::PATH))
            .expect("Valid URL")
            .join(&format!("{}/", Interface::PATH))
            .expect("Valid URL");

        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            format!("Bearer {}", *token.get().await)
                .parse()
                .expect("Valid header"),
        );

        let client = HttpClient::builder()
            .set_headers(headers)
            .build(&url)
            .with_context(|| format!("Failed to init client: {url}"))?;

        Ok(client)
    }

    async fn request<T>(&self, method: &str, params: impl ToRpcParams + Send) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let client = self.client().await?;
        Ok(client.request(method, params).await?)
    }
}

struct Inner {
    base_url: Url,
    token: TokenStore,
}

#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

impl Client {
    pub fn new(mut base_url: Url, token: TokenStore) -> Result<Self> {
        if !base_url.path().ends_with("/") {
            base_url.set_path(&format!("{}/", base_url.path()));
        }

        if !base_url.path().ends_with("/api/2/") {
            base_url.set_path(&format!("{}api/2/", base_url.path()));
        }

        let inner = Inner { base_url, token };

        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    pub fn url(&self) -> &Url {
        &self.inner.base_url
    }

    pub fn user(&self) -> UserNamespaceClient {
        self.derive()
    }

    pub async fn refresh_token(&self) -> Result<()> {
        let token = self.user().authentication().get_token().await?;
        self.inner.token.update(token).await
    }

    pub async fn socket(
        &self,
    ) -> Result<(sipsocket::Connection, mpsc::Receiver<ServerTransaction>)> {
        let slot = self
            .user()
            .slots()
            .get_all()
            .await?
            .into_iter()
            .find(|slot| slot.device_type == "webrtc")
            .context("No matching slot found")?;

        let (mut connection, requests) = sipsocket::Connection::connect(
            format!(
                "wss://{host}:{port}/sipsockets/",
                host = self.url().domain().expect("domain required"),
                port = slot.sip_port
            )
            .parse()
            .expect("valid URL"),
            &slot.sip_username,
        )
        .await?;

        connection
            .register(&slot.sip_username, &slot.sip_password)
            .await?;

        Ok((connection, requests))
    }
}
