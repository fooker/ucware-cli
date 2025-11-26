use crate::ucware::user::UserNamespace;
use crate::ucware::{Interface, InterfaceClient};
use anyhow::Result;
use jsonrpsee::rpc_params;

pub struct AuthenticationInterface;

impl Interface for AuthenticationInterface {
    const PATH: &'static str = "authentication";
}

pub type AuthenticationInterfaceClient = InterfaceClient<UserNamespace, AuthenticationInterface>;

impl AuthenticationInterfaceClient {
    pub async fn get_token(&self) -> Result<String> {
        self.request("getToken", rpc_params![]).await
    }

    pub async fn validate_token(&self) -> Result<String> {
        self.request("validateToken", rpc_params![]).await
    }
}
