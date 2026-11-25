use anyhow::{Result, anyhow};
use candid::Principal;
use ic_agent::Agent;
use ic_agent::identity::BasicIdentity;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::Mutex;

/// Thin wrapper over `ic-agent` for Taggr's hybrid JSON/Candid interface.
///
/// Taggr exposes most endpoints as custom `canister_query`/`canister_update`
/// methods whose raw argument bytes are JSON (see `src/backend/lib.rs`). A few
/// methods (`add_post`, `edit_post`) are proper Candid.
pub struct Ic {
    agent: Mutex<Agent>,
    canister: Principal,
}

impl Ic {
    pub async fn new(
        url: &str,
        canister_id: &str,
        identity: Option<BasicIdentity>,
    ) -> Result<Self> {
        let canister = Principal::from_text(canister_id)
            .map_err(|e| anyhow!("invalid canister id {canister_id}: {e}"))?;
        let mut builder = Agent::builder().with_url(url);
        if let Some(id) = identity {
            builder = builder.with_identity(id);
        }
        let agent = builder.build()?;
        if !url.contains("ic0.app") && !url.contains("icp0.io") && !url.contains("icp-api.io") {
            agent.fetch_root_key().await.ok();
        }
        Ok(Self {
            agent: Mutex::new(agent),
            canister,
        })
    }

    pub async fn set_identity(&self, identity: BasicIdentity) {
        let mut agent = self.agent.lock().await;
        agent.set_identity(identity);
    }

    pub fn canister_id(&self) -> String {
        self.canister.to_text()
    }

    pub async fn query_raw(&self, method: &str, arg: Vec<u8>) -> Result<Vec<u8>> {
        let agent = self.agent.lock().await;
        let reply = agent
            .query(&self.canister, method)
            .with_arg(arg)
            .call()
            .await?;
        Ok(reply)
    }

    pub async fn update_raw(&self, method: &str, arg: Vec<u8>) -> Result<Vec<u8>> {
        let agent = self.agent.lock().await;
        let reply = agent
            .update(&self.canister, method)
            .with_arg(arg)
            .call_and_wait()
            .await?;
        Ok(reply)
    }

    pub async fn query_json<T: DeserializeOwned>(
        &self,
        method: &str,
        args: Vec<Value>,
    ) -> Result<T> {
        let value = self.query_json_value(method, args).await?;
        serde_json::from_value(value).map_err(|e| anyhow!("decoding {method}: {e}"))
    }

    pub async fn query_json_value(&self, method: &str, args: Vec<Value>) -> Result<Value> {
        let arg = encode_args(&args)?;
        let reply = self.query_raw(method, arg).await?;
        decode_json(&reply, method)
    }

    pub async fn update_json<T: DeserializeOwned>(
        &self,
        method: &str,
        args: Vec<Value>,
    ) -> Result<T> {
        let value = self.update_json_value(method, args).await?;
        serde_json::from_value(value).map_err(|e| anyhow!("decoding {method}: {e}"))
    }

    pub async fn update_json_value(&self, method: &str, args: Vec<Value>) -> Result<Value> {
        let arg = encode_args(&args)?;
        let reply = self.update_raw(method, arg).await?;
        if reply.is_empty() {
            return Ok(Value::Null);
        }
        decode_json(&reply, method)
    }
}

/// Mirrors the frontend's `getEffParams`: 0 args -> null, 1 -> the arg, N -> array.
fn encode_args(args: &[Value]) -> Result<Vec<u8>> {
    let eff = match args.len() {
        0 => Value::Null,
        1 => args[0].clone(),
        _ => Value::Array(args.to_vec()),
    };
    Ok(serde_json::to_vec(&eff)?)
}

fn decode_json(reply: &[u8], method: &str) -> Result<Value> {
    if reply.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(reply).map_err(|e| {
        let snippet = String::from_utf8_lossy(&reply[..reply.len().min(200)]);
        anyhow!("{method}: invalid JSON reply ({e}): {snippet}")
    })
}

/// Serialize a value to JSON bytes (for the Candid `extension: opt blob` argument).
pub fn json_blob<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(value)?)
}
