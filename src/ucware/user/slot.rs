use crate::ucware::user::UserNamespace;
use crate::ucware::{Interface, InterfaceClient};
use anyhow::Result;
use jsonrpsee::rpc_params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Slot {
    pub id: u64,
    pub name: String,

    #[serde(rename = "userId")]
    pub user_id: u64,

    #[serde(rename = "deviceType")]
    pub device_type: String,

    #[serde(rename = "deviceId")]
    pub device_id: u64,

    #[serde(rename = "sipHost")]
    pub sip_host: String,

    #[serde(rename = "sipPort")]
    pub sip_port: u16,

    #[serde(rename = "sipUser")]
    pub sip_username: String,

    #[serde(rename = "sipPassword")]
    pub sip_password: String,

    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

pub struct SlotInterface;

impl Interface for SlotInterface {
    const PATH: &'static str = "slot";
}

pub type SlotInterfaceClient = InterfaceClient<UserNamespace, SlotInterface>;

impl SlotInterfaceClient {
    pub async fn get_all(&self) -> Result<Vec<Slot>> {
        self.request("getAll", rpc_params![]).await
    }
}
