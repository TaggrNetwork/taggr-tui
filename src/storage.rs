//! The user's media bucket: creation through the cycles minting canister, top-up,
//! upgrade, status and controllers (as in `src/frontend/src/user_storage.tsx`).

use crate::api::Api;
use anyhow::{Result, anyhow, bail};
use candid::{CandidType, Deserialize, Nat, Principal};
use serde_json::{Value, json};
use sha2::{Digest, Sha224};
use std::collections::BTreeMap;
use std::sync::Arc;

const CMC: &str = "rkp4c-7iaaa-aaaaa-aaaca-cai";
pub const BLACKHOLE: &str = "e3mmv-5qaaa-aaaah-aadma-cai";
const MEMO_CREATE: u64 = 0x41455243; // "CREA"
const MEMO_TOP_UP: u64 = 0x50555054; // "TPUP"
/// Saved in the user's settings so an interrupted creation can resume (shared with the web app).
const STATE_KEY: &str = "bucket_creation_state";

/// Legacy ICP account identifier of `owner` with `sub`, as hex.
pub fn account_id(owner: &Principal, sub: [u8; 32]) -> String {
    let mut h = Sha224::new();
    h.update(b"\x0Aaccount-id");
    h.update(owner.as_slice());
    h.update(sub);
    let hash = h.finalize();
    let mut out = crc32(&hash).to_be_bytes().to_vec();
    out.extend_from_slice(&hash);
    hex::encode(out)
}

/// Subaccount derived from a principal (length-prefixed bytes).
pub fn principal_sub(p: &Principal) -> [u8; 32] {
    let b = p.as_slice();
    let mut s = [0u8; 32];
    s[0] = b.len() as u8;
    s[1..1 + b.len()].copy_from_slice(b);
    s
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[derive(CandidType, Default)]
struct Settings {
    controllers: Option<Vec<Principal>>,
    compute_allocation: Option<Nat>,
    memory_allocation: Option<Nat>,
    freezing_threshold: Option<Nat>,
    reserved_cycles_limit: Option<Nat>,
    log_visibility: Option<LogVisibility>,
    wasm_memory_limit: Option<Nat>,
    wasm_memory_threshold: Option<Nat>,
}

#[derive(CandidType)]
#[allow(dead_code, non_camel_case_types)]
enum LogVisibility {
    controllers,
    public,
}

#[derive(CandidType)]
#[allow(dead_code)]
enum SubnetSelection {
    Subnet { subnet: Principal },
    Filter { subnet_type: Option<String> },
}

#[derive(CandidType)]
struct NotifyCreate {
    block_index: u64,
    controller: Principal,
    subnet_selection: Option<SubnetSelection>,
    settings: Option<Settings>,
}

#[derive(CandidType)]
struct NotifyTopUp {
    block_index: u64,
    canister_id: Principal,
}

#[derive(CandidType, Deserialize, Debug)]
enum NotifyError {
    Refunded {
        reason: String,
        block_index: Option<u64>,
    },
    InvalidTransaction(String),
    TransactionTooOld(u64),
    Processing,
    Other {
        error_code: u64,
        error_message: String,
    },
}

#[derive(CandidType)]
#[allow(non_camel_case_types)]
enum Mode {
    install,
    upgrade(Option<UpgradeOpts>),
}

#[derive(CandidType)]
struct UpgradeOpts {
    skip_pre_upgrade: Option<bool>,
    wasm_memory_persistence: Option<Persistence>,
}

#[derive(CandidType)]
#[allow(dead_code, non_camel_case_types)]
enum Persistence {
    keep,
    replace,
}

#[derive(CandidType)]
struct InstallCode {
    mode: Mode,
    canister_id: Principal,
    wasm_module: serde_bytes::ByteBuf,
    arg: serde_bytes::ByteBuf,
    sender_canister_version: Option<u64>,
}

#[derive(CandidType)]
struct CanisterId {
    canister_id: Principal,
}

#[derive(CandidType)]
struct UpdateSettings {
    canister_id: Principal,
    settings: Settings,
    sender_canister_version: Option<u64>,
}

#[derive(CandidType, Deserialize)]
#[allow(non_camel_case_types)]
enum Status {
    running,
    stopping,
    stopped,
}

#[derive(CandidType, Deserialize)]
struct DefiniteSettings {
    controllers: Vec<Principal>,
}

#[derive(CandidType, Deserialize)]
struct StatusReply {
    status: Status,
    settings: DefiniteSettings,
    module_hash: Option<serde_bytes::ByteBuf>,
    memory_size: Nat,
    cycles: Nat,
    idle_cycles_burned_per_day: Nat,
}

fn n(v: &Nat) -> u64 {
    v.0.to_string().parse().unwrap_or(u64::MAX)
}

async fn wasm(api: &Api) -> Result<Vec<u8>> {
    let (w,): (serde_bytes::ByteBuf,) = api
        .ic
        .candid_query(&api.ic.canister, "bucket_wasm", ())
        .await?;
    Ok(w.into_vec())
}

async fn save_state(
    api: &Api,
    settings: &mut BTreeMap<String, String>,
    state: Option<Value>,
) -> Result<()> {
    match state {
        Some(s) => settings.insert(STATE_KEY.into(), s.to_string()),
        None => settings.remove(STATE_KEY),
    };
    api.call("update_user_settings", vec![json!(settings)])
        .await?;
    Ok(())
}

/// Creates, installs and registers the bucket; resumes a saved attempt.
pub async fn create(
    api: &Arc<Api>,
    me: &str,
    e8s: u64,
    mut settings: BTreeMap<String, String>,
) -> Result<String> {
    let user = Principal::from_text(me)?;
    let cmc = Principal::from_text(CMC)?;
    let mut state: Value = settings
        .get(STATE_KEY)
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(Value::Null);
    if state.is_null() {
        let to = account_id(&cmc, principal_sub(&user));
        let block = api
            .icp_transfer(&to, e8s, MEMO_CREATE)
            .await
            .map_err(|e| anyhow!("ICP transfer to CMC failed: {e}"))?;
        state = json!({"stage": "transferred", "blockIndex": block.to_string()});
        save_state(api, &mut settings, Some(state.clone())).await?;
    }
    if state["stage"] == "transferred" {
        let block: u64 = state["blockIndex"].as_str().unwrap_or("0").parse()?;
        let arg = NotifyCreate {
            block_index: block,
            controller: user,
            subnet_selection: None,
            settings: Some(Settings {
                controllers: Some(vec![user, Principal::from_text(BLACKHOLE)?]),
                ..Default::default()
            }),
        };
        let (r,): (std::result::Result<Principal, NotifyError>,) = api
            .ic
            .candid_update(&cmc, "notify_create_canister", (arg,))
            .await?;
        let id = match r {
            Ok(id) => id,
            Err(NotifyError::Refunded { reason, .. }) => {
                save_state(api, &mut settings, None).await?;
                bail!("CMC refunded the payment ({reason}); try again");
            }
            Err(e) => bail!("CMC reported {e:?}"),
        };
        state = json!({"stage": "created", "canisterId": id.to_text()});
        save_state(api, &mut settings, Some(state.clone())).await?;
    }
    let id = Principal::from_text(state["canisterId"].as_str().unwrap_or_default())?;
    if state["stage"] == "created" {
        let init = candid::encode_args((vec![user],))?;
        let arg = InstallCode {
            mode: Mode::install,
            canister_id: id,
            wasm_module: serde_bytes::ByteBuf::from(wasm(api).await?),
            arg: serde_bytes::ByteBuf::from(init),
            sender_canister_version: None,
        };
        api.ic
            .mgmt_update(&id, "install_code", candid::encode_args((arg,))?)
            .await?;
        state = json!({"stage": "installed", "canisterId": id.to_text()});
        save_state(api, &mut settings, Some(state.clone())).await?;
    }
    api.call("set_bucket", vec![json!(id.to_text())]).await?;
    save_state(api, &mut settings, None).await?;
    Ok(id.to_text())
}

pub async fn top_up(api: &Arc<Api>, bucket: &str, e8s: u64) -> Result<u64> {
    let id = Principal::from_text(bucket)?;
    let cmc = Principal::from_text(CMC)?;
    let to = account_id(&cmc, principal_sub(&id));
    let block = api
        .icp_transfer(&to, e8s, MEMO_TOP_UP)
        .await
        .map_err(|e| anyhow!("ICP transfer to CMC failed: {e}"))?;
    let (r,): (std::result::Result<Nat, NotifyError>,) = api
        .ic
        .candid_update(
            &cmc,
            "notify_top_up",
            (NotifyTopUp {
                block_index: block,
                canister_id: id,
            },),
        )
        .await?;
    r.map(|c| n(&c))
        .map_err(|e| anyhow!("CMC top-up reported {e:?}"))
}

pub async fn upgrade(api: &Arc<Api>, bucket: &str) -> Result<()> {
    let id = Principal::from_text(bucket)?;
    let arg = InstallCode {
        mode: Mode::upgrade(None),
        canister_id: id,
        wasm_module: serde_bytes::ByteBuf::from(wasm(api).await?),
        arg: serde_bytes::ByteBuf::new(),
        sender_canister_version: None,
    };
    api.ic
        .mgmt_update(&id, "install_code", candid::encode_args((arg,))?)
        .await?;
    Ok(())
}

async fn controllers(api: &Arc<Api>, id: &Principal) -> Result<Vec<Principal>> {
    let r = api
        .ic
        .mgmt_update(
            id,
            "canister_status",
            candid::encode_args((CanisterId { canister_id: *id },))?,
        )
        .await?;
    let (s,): (StatusReply,) = candid::decode_args(&r)?;
    Ok(s.settings.controllers)
}

/// Adds `who` as a controller of the bucket (management canister and the bucket's own list).
pub async fn add_controller(api: &Arc<Api>, bucket: &str, who: &str) -> Result<()> {
    let id = Principal::from_text(bucket)?;
    let added = Principal::from_text(who)?;
    let mut list: Vec<Principal> = controllers(api, &id)
        .await?
        .into_iter()
        .filter(|p| *p != added)
        .collect();
    list.push(added);
    let arg = UpdateSettings {
        canister_id: id,
        settings: Settings {
            controllers: Some(list.clone()),
            ..Default::default()
        },
        sender_canister_version: None,
    };
    api.ic
        .mgmt_update(&id, "update_settings", candid::encode_args((arg,))?)
        .await?;
    api.ic
        .update_raw(
            &id,
            "update_internal_controllers",
            candid::encode_args((list,))?,
        )
        .await?;
    if !controllers(api, &id).await?.contains(&added) {
        bail!("new principal not found in bucket controllers after update");
    }
    Ok(())
}

/// Status for the settings page: status, cycles, memory, days left, controllers, stale code.
pub async fn status(api: &Arc<Api>, bucket: &str) -> Result<Value> {
    let id = Principal::from_text(bucket)?;
    let r = api
        .ic
        .mgmt_update(
            &id,
            "canister_status",
            candid::encode_args((CanisterId { canister_id: id },))?,
        )
        .await?;
    let (s,): (StatusReply,) = candid::decode_args(&r)?;
    let want: String = api
        .ic
        .query("bucket_wasm_hash", vec![])
        .await
        .unwrap_or_default();
    let have = s
        .module_hash
        .map(|h| hex::encode(h.as_slice()))
        .unwrap_or_default();
    let burn = n(&s.idle_cycles_burned_per_day);
    let cycles = n(&s.cycles);
    Ok(json!({
        "status": match s.status { Status::running => "running", Status::stopping => "stopping", Status::stopped => "stopped" },
        "cycles": cycles,
        "memory": n(&s.memory_size),
        "days": cycles.checked_div(burn),
        "controllers": s.settings.controllers.iter().map(|p| p.to_text()).collect::<Vec<_>>(),
        "stale": !want.is_empty() && !have.is_empty() && want != have,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_ids() {
        // the anonymous principal's default account, a well-known value
        let anon = Principal::anonymous();
        assert_eq!(
            account_id(&anon, [0; 32]),
            "1c7a48ba6a562aa9eaa2481a9049cdf0433b9738c992d698c31d8abf89cadc79"
        );
    }
}
