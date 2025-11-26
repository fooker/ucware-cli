use crate::ucware::user::authentication::AuthenticationInterfaceClient;
use crate::ucware::{Derive, Namespace, NamespaceClient};
use crate::ucware::user::slot::SlotInterfaceClient;

mod authentication;
mod slot;

pub struct UserNamespace;

impl Namespace for UserNamespace {
    const PATH: &'static str = "user";
}

pub type UserNamespaceClient = NamespaceClient<UserNamespace>;

impl UserNamespaceClient {
    pub fn authentication(&self) -> AuthenticationInterfaceClient {
        self.derive()
    }

    pub fn slots(&self) -> SlotInterfaceClient {
        self.derive()
    }
}
