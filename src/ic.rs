//! Thin wrapper over `ic-agent` for Taggr's hybrid JSON/Candid interface.
//!
//! Most Taggr endpoints are raw `canister_query`/`canister_update` exports whose
//! argument and reply bytes are JSON (`src/backend/queries.rs`, `updates.rs`).
//! `add_post`, `edit_post`, the ICRC-1 methods and other canisters (ICP ledger,
//! cycles minting canister, management canister, media buckets) are Candid.

use anyhow::{Result, anyhow};
use candid::Principal;
use candid::utils::{ArgumentDecoder, ArgumentEncoder};
use ic_agent::Agent;
use ic_agent::identity::BasicIdentity;
use serde::de::DeserializeOwned;
use serde_json::Value;

pub struct Ic {
    agent: Agent,
    pub canister: Principal,
}

impl Ic {
    pub async fn new(url: &str, canister: &str, identity: Option<BasicIdentity>) -> Result<Self> {
        let canister = Principal::from_text(canister)
            .map_err(|e| anyhow!("invalid canister id {canister}: {e}"))?;
        let mut builder = Agent::builder().with_url(url);
        if let Some(id) = identity {
            builder = builder.with_identity(id);
        }
        let agent = builder.build()?;
        if !url.contains("ic0.app") && !url.contains("icp0.io") && !url.contains("icp-api.io") {
            agent.fetch_root_key().await?;
        }
        Ok(Self { agent, canister })
    }

    pub async fn query_raw(
        &self,
        canister: &Principal,
        method: &str,
        arg: Vec<u8>,
    ) -> Result<Vec<u8>> {
        Ok(self
            .agent
            .query(canister, method)
            .with_arg(arg)
            .call()
            .await?)
    }

    pub async fn update_raw(
        &self,
        canister: &Principal,
        method: &str,
        arg: Vec<u8>,
    ) -> Result<Vec<u8>> {
        Ok(self
            .agent
            .update(canister, method)
            .with_arg(arg)
            .call_and_wait()
            .await?)
    }

    /// Management canister calls must name the canister they act on as the effective id.
    pub async fn mgmt_update(
        &self,
        effective: &Principal,
        method: &str,
        arg: Vec<u8>,
    ) -> Result<Vec<u8>> {
        Ok(self
            .agent
            .update(&Principal::management_canister(), method)
            .with_effective_canister_id(*effective)
            .with_arg(arg)
            .call_and_wait()
            .await?)
    }

    pub async fn query<T: DeserializeOwned>(&self, method: &str, args: Vec<Value>) -> Result<T> {
        let reply = self
            .query_raw(&self.canister, method, encode_args(&args)?)
            .await?;
        decode_json(&reply, method)
    }

    pub async fn update<T: DeserializeOwned>(&self, method: &str, args: Vec<Value>) -> Result<T> {
        let reply = self
            .update_raw(&self.canister, method, encode_args(&args)?)
            .await?;
        if reply.is_empty() {
            return serde_json::from_value(Value::Null).map_err(|e| anyhow!("{method}: {e}"));
        }
        decode_json(&reply, method)
    }

    /// JSON update replying `{"Ok": T}` or `{"Err": "…"}`.
    pub async fn update_ok<T: DeserializeOwned>(
        &self,
        method: &str,
        args: Vec<Value>,
    ) -> Result<T> {
        let v: Value = self.update(method, args).await?;
        unwrap_result(v, method)
    }

    pub async fn candid_query<A, R>(&self, canister: &Principal, method: &str, args: A) -> Result<R>
    where
        A: ArgumentEncoder,
        R: for<'a> ArgumentDecoder<'a>,
    {
        let arg = candid::encode_args(args)?;
        let reply = self.query_raw(canister, method, arg).await?;
        Ok(candid::decode_args(&reply)?)
    }

    pub async fn candid_update<A, R>(
        &self,
        canister: &Principal,
        method: &str,
        args: A,
    ) -> Result<R>
    where
        A: ArgumentEncoder,
        R: for<'a> ArgumentDecoder<'a>,
    {
        let arg = candid::encode_args(args)?;
        let reply = self.update_raw(canister, method, arg).await?;
        Ok(candid::decode_args(&reply)?)
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

fn decode_json<T: DeserializeOwned>(reply: &[u8], method: &str) -> Result<T> {
    serde_json::from_slice(reply).map_err(|e| {
        let snippet = String::from_utf8_lossy(&reply[..reply.len().min(200)]);
        anyhow!("{method}: unexpected reply ({e}): {snippet}")
    })
}

pub fn unwrap_result<T: DeserializeOwned>(v: Value, method: &str) -> Result<T> {
    match v {
        Value::Object(mut m) if m.len() == 1 && m.contains_key("Err") => {
            let e = m.remove("Err").unwrap_or_default();
            Err(anyhow!(
                "{}",
                e.as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| e.to_string())
            ))
        }
        Value::Object(mut m) if m.len() == 1 && m.contains_key("Ok") => {
            serde_json::from_value(m.remove("Ok").unwrap_or_default())
                .map_err(|e| anyhow!("{method}: {e}"))
        }
        other => serde_json::from_value(other).map_err(|e| anyhow!("{method}: {e}")),
    }
}
