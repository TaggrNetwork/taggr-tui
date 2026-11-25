pub mod models;
pub mod reads;
pub mod writes;

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::ic::Ic;

pub struct Client {
    pub ic: Arc<Ic>,
    pub domain: String,
    pub raw_suffix: String,
}

impl Client {
    pub fn new(ic: Arc<Ic>, domain: String, raw_suffix: String) -> Self {
        Self {
            ic,
            domain,
            raw_suffix,
        }
    }

    pub fn image_url(&self, bucket: &str, offset: u64, len: u64) -> String {
        format!(
            "https://{bucket}.{}/image?offset={offset}&len={len}",
            self.raw_suffix
        )
    }
}

pub fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

pub fn arg<T: Into<Value>>(value: T) -> Value {
    value.into()
}
