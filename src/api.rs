//! Typed access to the Taggr canister and the ledgers it works with.
//! Shapes follow `src/backend/{queries,updates}.rs` and `src/frontend/src/types.tsx`.

use crate::ic::Ic;
use anyhow::{Result, anyhow, bail};
use candid::{CandidType, Nat, Principal};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub type PostId = u64;
pub type UserId = u64;

pub const ICP_LEDGER: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";
pub const ICP_FEE: u64 = 10_000;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum Extension {
    Poll(Poll),
    Repost(PostId),
    Proposal(u32),
    Feature,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Poll {
    pub options: Vec<String>,
    pub votes: BTreeMap<u16, Vec<UserId>>,
    pub voters: Vec<UserId>,
    pub deadline: u64,
    pub weighted_by_karma: BTreeMap<u16, i64>,
    pub weighted_by_tokens: BTreeMap<u16, u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Post {
    pub id: PostId,
    pub parent: Option<PostId>,
    pub watchers: Vec<UserId>,
    pub children: Vec<PostId>,
    pub reposts: Vec<PostId>,
    pub user: UserId,
    pub body: String,
    pub reactions: BTreeMap<u16, Vec<UserId>>,
    pub files: BTreeMap<String, (u64, u64)>,
    pub patches: Vec<(u64, String)>,
    pub tips: Vec<(UserId, u64)>,
    pub hashes: Vec<String>,
    pub realm: Option<String>,
    pub timestamp: u64,
    pub extension: Option<Extension>,
    pub tree_size: u64,
    pub tree_update: u64,
    pub encrypted: bool,
    pub hidden_for: Vec<UserId>,
}

impl Post {
    pub fn poll(&self) -> Option<&Poll> {
        match &self.extension {
            Some(Extension::Poll(p)) => Some(p),
            _ => None,
        }
    }
    pub fn deleted(&self) -> bool {
        !self.hashes.is_empty()
    }
    /// `(blob id, url)` of every attached picture.
    pub fn images(&self, local: Option<&str>) -> BTreeMap<String, String> {
        self.files
            .iter()
            .filter_map(|(k, (off, len))| {
                let (id, bucket) = k.split_once('@')?;
                Some((id.to_string(), image_url(bucket, *off, *len, local)))
            })
            .collect()
    }
}

pub fn image_url(bucket: &str, offset: u64, len: u64, local: Option<&str>) -> String {
    let host = match local {
        None => format!("https://{bucket}.raw.icp0.io"),
        Some(h) => format!("http://{bucket}.raw.{h}"),
    };
    format!("{host}/image?offset={offset}&len={len}")
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Meta {
    pub author_name: String,
    pub viewer_blocked: bool,
    pub realm_color: Option<String>,
    pub nsfw: bool,
    pub max_downvotes_reached: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Item {
    pub post: Post,
    pub meta: Meta,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct UserFilter {
    pub age_days: u64,
    pub safe: bool,
    pub balance: u64,
    pub num_followers: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Filters {
    pub users: Vec<UserId>,
    pub tags: Vec<String>,
    pub realms: Vec<String>,
    pub noise: UserFilter,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Report {
    pub reason: String,
    pub reporter: UserId,
    pub confirmed_by: Vec<UserId>,
    pub rejected_by: Vec<UserId>,
    pub closed: bool,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub enum Predicate {
    ReportOpen(PostId),
    UserReportOpen(UserId),
    Proposal(PostId),
}

#[derive(Debug, Clone, Deserialize)]
pub enum Notification {
    Generic(String),
    NewPost(String, PostId),
    Conditional(String, Predicate),
    WatchedPostEntries(PostId, Vec<PostId>),
}

impl Notification {
    /// The message as Markdown, as the inbox shows it.
    pub fn text(&self) -> String {
        match self {
            Notification::Generic(m)
            | Notification::NewPost(m, _)
            | Notification::Conditional(m, _) => m.clone(),
            Notification::WatchedPostEntries(p, ids) => {
                format!("{} new thread update(s) on watched post #{p}", ids.len())
            }
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct User {
    pub id: UserId,
    pub name: String,
    pub num_posts: u64,
    pub bookmarks: Vec<PostId>,
    pub pinned_posts: Vec<PostId>,
    pub about: String,
    pub account: String,
    pub settings: BTreeMap<String, String>,
    pub cold_wallet: Option<String>,
    pub cycles: i64,
    pub rewards: i64,
    pub feeds: Vec<Vec<String>>,
    pub followees: Vec<UserId>,
    pub followers: Vec<UserId>,
    pub timestamp: u64,
    pub last_activity: u64,
    pub stalwart: bool,
    pub invited_by: Option<UserId>,
    pub accounting: Vec<(u64, String, i64, String)>,
    pub realms: Vec<String>,
    pub balance: u64,
    pub cold_balance: u64,
    pub active_weeks: u32,
    pub principal: String,
    pub bucket: Option<String>,
    pub report: Option<Report>,
    pub blacklist: Vec<UserId>,
    pub treasury_e8s: u64,
    pub filters: Filters,
    pub previous_names: Vec<String>,
    pub governance: bool,
    pub notifications: BTreeMap<u64, (Notification, bool)>,
    pub show_posts_in_realms: bool,
    pub controlled_realms: Vec<String>,
    pub mode: String,
    pub deactivated: bool,
    pub wallet_tokens: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Realm {
    pub cleanup_penalty: u64,
    pub controllers: Vec<UserId>,
    pub description: String,
    pub filter: UserFilter,
    pub label_color: String,
    pub last_setting_update: u64,
    pub last_update: u64,
    pub logo: String,
    pub max_downvotes: u32,
    pub num_members: u64,
    pub num_posts: u64,
    pub revenue: u64,
    pub theme: String,
    pub whitelist: Vec<UserId>,
    pub created: u64,
    pub posts: Vec<PostId>,
    pub adult_content: bool,
    pub comments_filtering: bool,
}

/// Backend constants (`config` query); only what the TUI uses is typed.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub name: String,
    pub token_symbol: String,
    pub token_decimals: u32,
    pub transaction_fee: u64,
    pub post_cost: i64,
    pub poll_cost: i64,
    pub post_deletion_penalty_factor: i64,
    pub max_post_length: usize,
    pub max_blob_size_bytes: usize,
    pub max_report_length: usize,
    pub reporting_penalty_misbehaviour: i64,
    pub identity_change_cost: i64,
    pub feed_page_size: usize,
    pub reactions: Vec<(u16, i64)>,
    pub max_credits_mint_kilos: u64,
    pub proposal_escrow_amount_xdr: u64,
    pub poll_revote_deadline_hours: u64,
    pub max_realm_name: usize,
    pub max_realm_logo_len: usize,
    pub realm_cost: i64,
    pub min_credits_for_inviting: i64,
    pub credits_per_xdr: u64,
    pub maximum_supply: u64,
    pub weekly_auction_size_tokens: u64,
    pub account_activation_cost: i64,
    #[serde(flatten)]
    pub rest: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum DomainSub {
    Journal(UserId),
    BlackListedRealms(Vec<String>),
    WhiteListedRealms(Vec<String>),
}

impl Default for DomainSub {
    fn default() -> Self {
        DomainSub::BlackListedRealms(vec![])
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct DomainConfig {
    pub owner: Option<UserId>,
    pub sub_config: DomainSub,
    pub max_downvotes: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Invite {
    pub credits: i64,
    pub credits_per_user: i64,
    pub joined_user_ids: Vec<UserId>,
    pub realm_id: Option<String>,
    pub inviter_user_id: UserId,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SearchResult {
    pub id: PostId,
    pub user_id: UserId,
    pub generic_id: String,
    pub result: String,
    pub relevant: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Account {
    pub owner: String,
    pub subaccount: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Transaction {
    pub timestamp: u64,
    pub from: Account,
    pub to: Account,
    pub amount: u64,
    pub fee: u64,
    pub memo: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Invoice {
    pub e8s: u64,
    pub paid_e8s: u64,
    pub paid: bool,
    pub account: Value,
}

impl Invoice {
    /// The ICP account to pay to, as hex.
    pub fn account_hex(&self) -> String {
        account_hex(&self.account)
    }
}

pub fn account_hex(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => hex::encode(
            a.iter()
                .filter_map(|b| b.as_u64())
                .map(|b| b as u8)
                .collect::<Vec<_>>(),
        ),
        _ => String::new(),
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Bid {
    pub user: UserId,
    pub amount: u64,
    pub e8s_per_token: u64,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Auction {
    pub amount: u64,
    pub bids: Vec<Bid>,
}

/// Everything shared by all calls: the agent, the domain, and the canister config.
pub struct Api {
    pub ic: Ic,
    pub domain: String,
    /// `host:port` of a local replica; None on mainnet.
    pub local: Option<String>,
}

type Pairs = Vec<(Post, Meta)>;

fn items(v: Pairs) -> Vec<Item> {
    v.into_iter()
        .map(|(post, meta)| Item { post, meta })
        .collect()
}

#[derive(CandidType, Deserialize)]
enum CandidResult<T> {
    Ok(T),
    Err(String),
}

impl<T> CandidResult<T> {
    fn into_result(self) -> Result<T> {
        match self {
            CandidResult::Ok(v) => Ok(v),
            CandidResult::Err(e) => Err(anyhow!(e)),
        }
    }
}

#[derive(CandidType, Deserialize, Clone)]
pub struct IcrcAccount {
    pub owner: Principal,
    pub subaccount: Option<serde_bytes::ByteBuf>,
}

#[derive(CandidType)]
struct IcrcTransferArg {
    to: IcrcAccount,
    fee: Option<Nat>,
    memo: Option<serde_bytes::ByteBuf>,
    from_subaccount: Option<serde_bytes::ByteBuf>,
    created_at_time: Option<u64>,
    amount: Nat,
}

#[derive(CandidType, Deserialize, Debug)]
pub enum IcrcTransferError {
    BadFee { expected_fee: Nat },
    BadBurn { min_burn_amount: Nat },
    InsufficientFunds { balance: Nat },
    TooOld,
    CreatedInFuture { ledger_time: u64 },
    TemporarilyUnavailable,
    Duplicate { duplicate_of: Nat },
    GenericError { error_code: Nat, message: String },
}

#[derive(CandidType, Deserialize)]
pub enum IcrcValue {
    Nat(Nat),
    Int(candid::Int),
    Text(String),
    Blob(serde_bytes::ByteBuf),
}

#[derive(CandidType, Deserialize)]
struct Tokens {
    e8s: u64,
}

#[derive(CandidType)]
struct IcpTransferArgs {
    memo: u64,
    amount: Tokens,
    fee: Tokens,
    from_subaccount: Option<serde_bytes::ByteBuf>,
    to: serde_bytes::ByteBuf,
    created_at_time: Option<Timestamp>,
}

#[derive(CandidType)]
struct Timestamp {
    timestamp_nanos: u64,
}

#[derive(CandidType, Deserialize, Debug)]
enum IcpTransferError {
    BadFee { expected_fee: Tokens0 },
    InsufficientFunds { balance: Tokens0 },
    TxTooOld { allowed_window_nanos: u64 },
    TxCreatedInFuture,
    TxDuplicate { duplicate_of: u64 },
}

#[derive(CandidType, Deserialize, Debug)]
struct Tokens0 {
    e8s: u64,
}

pub struct IcrcMeta {
    pub name: String,
    pub symbol: String,
    pub decimals: u32,
    pub fee: u64,
}

fn nat_u64(n: &Nat) -> u64 {
    n.0.to_string().parse().unwrap_or(u64::MAX)
}

impl Api {
    fn d(&self) -> Value {
        json!(self.domain)
    }

    // ---- reads ---------------------------------------------------------------

    pub async fn config_raw(&self) -> Result<Value> {
        self.ic.query("config", vec![]).await
    }
    pub async fn stats(&self) -> Result<Value> {
        self.ic.query("stats", vec![]).await
    }
    pub async fn domains(&self) -> Result<BTreeMap<String, DomainConfig>> {
        self.ic.query("domains", vec![]).await
    }
    pub async fn user(&self, handle: Option<&str>) -> Result<Option<User>> {
        let list: Vec<Value> = handle.map(|h| vec![json!(h)]).unwrap_or_default();
        self.ic.query("user", vec![self.d(), json!(list)]).await
    }
    pub async fn users_data(&self, ids: &[UserId]) -> Result<BTreeMap<UserId, String>> {
        self.ic.query("users_data", vec![json!(ids)]).await
    }
    pub async fn last_posts(&self, realm: &str, page: usize, offset: PostId) -> Result<Vec<Item>> {
        let v: Pairs = self
            .ic
            .query(
                "last_posts",
                vec![
                    self.d(),
                    json!(realm),
                    json!(page),
                    json!(offset),
                    json!(true),
                ],
            )
            .await?;
        Ok(items(v))
    }
    pub async fn hot_posts(&self, realm: &str, page: usize, offset: PostId) -> Result<Vec<Item>> {
        let v: Pairs = self
            .ic
            .query(
                "hot_posts",
                vec![
                    self.d(),
                    json!(realm),
                    json!(page),
                    json!(offset),
                    json!(true),
                ],
            )
            .await?;
        Ok(items(v))
    }
    pub async fn personal_feed(&self, page: usize, offset: PostId) -> Result<Vec<Item>> {
        let v: Pairs = self
            .ic
            .query("personal_feed", vec![self.d(), json!(page), json!(offset)])
            .await?;
        Ok(items(v))
    }
    pub async fn posts_by_tags(
        &self,
        realm: &str,
        tags: &[String],
        page: usize,
        offset: PostId,
    ) -> Result<Vec<Item>> {
        let v: Pairs = self
            .ic
            .query(
                "posts_by_tags",
                vec![
                    self.d(),
                    json!(realm),
                    json!(tags),
                    json!(page),
                    json!(offset),
                ],
            )
            .await?;
        Ok(items(v))
    }
    /// `method`: journal | user_posts | rewarded_posts | user_tags.
    pub async fn user_feed(
        &self,
        method: &str,
        handle: &str,
        page: usize,
        offset: PostId,
    ) -> Result<Vec<Item>> {
        let v: Option<Pairs> = self
            .ic
            .query(
                method,
                vec![self.d(), json!(handle), json!(page), json!(offset)],
            )
            .await?;
        Ok(items(v.unwrap_or_default()))
    }
    pub async fn posts(&self, ids: &[PostId]) -> Result<Vec<Item>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let v: Pairs = self.ic.query("posts", vec![json!(ids)]).await?;
        Ok(items(v))
    }
    pub async fn thread(&self, id: PostId) -> Result<Vec<Item>> {
        let v: Pairs = self.ic.query("thread", vec![json!(id)]).await?;
        Ok(items(v))
    }
    pub async fn search(&self, q: &str) -> Result<Vec<SearchResult>> {
        self.ic.query("search", vec![self.d(), json!(q)]).await
    }
    pub async fn realms(&self, ids: &[String]) -> Result<Vec<Realm>> {
        self.ic.query("realms", vec![json!(ids)]).await
    }
    pub async fn all_realms(&self, order: &str, page: usize) -> Result<Vec<(String, Realm)>> {
        self.ic
            .query("all_realms", vec![self.d(), json!(order), json!(page)])
            .await
    }
    pub async fn realm_search(&self, order: &str, q: &str) -> Result<Vec<(String, Realm)>> {
        self.ic
            .query("realm_search", vec![self.d(), json!(order), json!(q)])
            .await
    }
    pub async fn invites(&self) -> Result<Vec<(String, Invite)>> {
        self.ic.query("invites", vec![]).await
    }
    pub async fn proposals(&self, page: usize, filter: &str) -> Result<Vec<Item>> {
        let v: Pairs = self
            .ic
            .query("proposals", vec![json!(page), json!(filter)])
            .await?;
        Ok(items(v))
    }
    pub async fn proposal(&self, id: u32) -> Result<Value> {
        let v: Value = self.ic.query("proposal", vec![json!(id)]).await?;
        crate::ic::unwrap_result(v, "proposal")
    }
    pub async fn token_stats(&self) -> Result<Value> {
        self.ic.query("token_stats", vec![]).await
    }
    pub async fn transactions(
        &self,
        page: usize,
        principal: &str,
        subaccount: &str,
    ) -> Result<Vec<(u32, Transaction)>> {
        self.ic
            .query(
                "transactions",
                vec![json!(page), json!(principal), json!(subaccount)],
            )
            .await
    }
    pub async fn transaction(&self, id: u32) -> Result<Transaction> {
        let v: Value = self.ic.query("transaction", vec![json!(id)]).await?;
        crate::ic::unwrap_result(v, "transaction")
    }
    pub async fn auction(&self) -> Result<(Auction, String)> {
        self.ic.query("auction", vec![]).await
    }
    pub async fn distribution(&self) -> Result<Vec<Value>> {
        self.ic.query("distribution", vec![]).await
    }
    pub async fn logs(&self) -> Result<Vec<Value>> {
        self.ic.query("logs", vec![]).await
    }
    pub async fn validate_username(&self, name: &str) -> Result<()> {
        let v: Value = self
            .ic
            .query("validate_username", vec![json!(name)])
            .await?;
        crate::ic::unwrap_result::<Value>(v, "validate_username").map(|_| ())
    }
    pub async fn check_invite(&self, code: &str) -> Result<bool> {
        self.ic.query("check_invite", vec![json!(code)]).await
    }
    pub async fn validate_proposal(&self, payload: &Value) -> Result<()> {
        let v: Value = self
            .ic
            .query("validate_proposal", vec![payload.clone()])
            .await?;
        crate::ic::unwrap_result::<Value>(v, "validate_proposal").map(|_| ())
    }
    pub async fn migration_pending(&self) -> Result<bool> {
        self.ic.query("migration_pending", vec![]).await
    }
    pub async fn principal_change_target(&self) -> Result<Option<String>> {
        self.ic.query("principal_change_target", vec![]).await
    }

    // ---- writes (JSON) ---------------------------------------------------------

    /// Any JSON update replying Ok/Err, for the many one-line actions.
    pub async fn call(&self, method: &str, args: Vec<Value>) -> Result<Value> {
        self.ic.update_ok(method, args).await
    }

    /// Toggles that reply with a bare value, not Ok/Err (e.g. `toggle_pinned_post` replies `null`).
    pub async fn call_any(&self, method: &str, args: Vec<Value>) -> Result<Value> {
        let v: Value = self.ic.update(method, args).await?;
        crate::ic::unwrap_result(v, method)
    }

    // ---- writes (Candid) ---------------------------------------------------------

    pub async fn add_post(
        &self,
        body: &str,
        refs: Vec<(String, u64, u64)>,
        parent: Option<PostId>,
        realm: Option<String>,
        extension: Option<&Extension>,
    ) -> Result<PostId> {
        let ext = extension
            .map(|e| serde_bytes::ByteBuf::from(serde_json::to_vec(e).unwrap_or_default()));
        let (r,): (CandidResult<u64>,) = self
            .ic
            .candid_update(
                &self.ic.canister,
                "add_post",
                (body.to_string(), refs, parent, realm, ext),
            )
            .await?;
        r.into_result()
    }

    pub async fn edit_post(
        &self,
        id: PostId,
        body: &str,
        refs: Vec<(String, u64, u64)>,
        patch: String,
        realm: Option<String>,
    ) -> Result<()> {
        let (r,): (CandidResult<()>,) = self
            .ic
            .candid_update(
                &self.ic.canister,
                "edit_post",
                (id, body.to_string(), refs, patch, realm),
            )
            .await?;
        r.into_result()
    }

    pub async fn unlink_cold_wallet(&self) -> Result<()> {
        let (r,): (CandidResult<()>,) = self
            .ic
            .candid_update(&self.ic.canister, "unlink_cold_wallet", ())
            .await?;
        r.into_result()
    }

    pub async fn propose_release(
        &self,
        post: PostId,
        commit: String,
        binary: Vec<u8>,
    ) -> Result<u32> {
        let (r,): (CandidResult<u32>,) = self
            .ic
            .candid_update(
                &self.ic.canister,
                "propose_release",
                (post, commit, serde_bytes::ByteBuf::from(binary)),
            )
            .await?;
        r.into_result()
    }

    // ---- ledgers ---------------------------------------------------------------

    pub async fn icrc_balance(&self, token: &Principal, owner: Principal) -> Result<u64> {
        let (n,): (Nat,) = self
            .ic
            .candid_query(
                token,
                "icrc1_balance_of",
                (IcrcAccount {
                    owner,
                    subaccount: None,
                },),
            )
            .await?;
        Ok(nat_u64(&n))
    }

    pub async fn icrc_transfer(
        &self,
        token: &Principal,
        to: IcrcAccount,
        amount: u64,
        fee: u64,
    ) -> Result<u64> {
        let arg = IcrcTransferArg {
            to,
            fee: Some(Nat::from(fee)),
            memo: None,
            from_subaccount: None,
            created_at_time: None,
            amount: Nat::from(amount),
        };
        let (r,): (std::result::Result<Nat, IcrcTransferError>,) = self
            .ic
            .candid_update(token, "icrc1_transfer", (arg,))
            .await?;
        r.map(|n| nat_u64(&n))
            .map_err(|e| anyhow!("transfer failed: {e:?}"))
    }

    pub async fn icrc_meta(&self, token: &Principal) -> Result<IcrcMeta> {
        let (m,): (Vec<(String, IcrcValue)>,) =
            self.ic.candid_query(token, "icrc1_metadata", ()).await?;
        let mut out = IcrcMeta {
            name: String::new(),
            symbol: String::new(),
            decimals: 8,
            fee: 0,
        };
        for (k, v) in m {
            match (k.as_str(), v) {
                ("icrc1:name", IcrcValue::Text(t)) => out.name = t,
                ("icrc1:symbol", IcrcValue::Text(t)) => out.symbol = t,
                ("icrc1:decimals", IcrcValue::Nat(n)) => out.decimals = nat_u64(&n) as u32,
                ("icrc1:fee", IcrcValue::Nat(n)) => out.fee = nat_u64(&n),
                _ => {}
            }
        }
        Ok(out)
    }

    pub async fn icp_balance(&self, owner: Principal) -> Result<u64> {
        let ledger = Principal::from_text(ICP_LEDGER)?;
        self.icrc_balance(&ledger, owner).await
    }

    /// Legacy ICP transfer to a hex account identifier; returns the block index.
    pub async fn icp_transfer(&self, account_hex: &str, e8s: u64, memo: u64) -> Result<u64> {
        let to = hex::decode(account_hex.trim()).map_err(|_| anyhow!("invalid account id"))?;
        if to.len() != 32 {
            bail!("account id must be 32 bytes (64 hex digits)");
        }
        let ledger = Principal::from_text(ICP_LEDGER)?;
        let arg = IcpTransferArgs {
            memo,
            amount: Tokens { e8s },
            fee: Tokens { e8s: ICP_FEE },
            from_subaccount: None,
            to: serde_bytes::ByteBuf::from(to),
            created_at_time: None,
        };
        let (r,): (std::result::Result<u64, IcpTransferError>,) =
            self.ic.candid_update(&ledger, "transfer", (arg,)).await?;
        r.map_err(|e| anyhow!("ICP transfer failed: {e:?}"))
    }

    // ---- media bucket ------------------------------------------------------------

    /// Stores raw bytes in a bucket; returns the offset.
    pub async fn bucket_write(&self, bucket: &Principal, blob: Vec<u8>) -> Result<u64> {
        let r = self.ic.update_raw(bucket, "write", blob).await?;
        if r.len() < 8 {
            bail!("bucket write: short reply");
        }
        Ok(u64::from_be_bytes(r[..8].try_into()?))
    }

    pub async fn bucket_free(&self, bucket: &Principal, segments: Vec<(u64, u64)>) -> Result<()> {
        if segments.is_empty() {
            return Ok(());
        }
        let arg = candid::encode_args((segments,))?;
        self.ic.update_raw(bucket, "free", arg).await?;
        Ok(())
    }
}
