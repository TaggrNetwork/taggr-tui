use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub type PostId = u64;
pub type UserId = u64;

#[derive(Debug, Clone, Deserialize)]
pub struct Poll {
    pub options: Vec<String>,
    #[serde(default)]
    pub votes: BTreeMap<u16, Vec<UserId>>,
    #[serde(default)]
    pub voters: Vec<UserId>,
    #[serde(default)]
    pub deadline: u64,
    #[serde(default)]
    pub weighted_by_karma: BTreeMap<u16, i64>,
    #[serde(default)]
    pub weighted_by_tokens: BTreeMap<u16, u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub enum Extension {
    Poll(Poll),
    Repost(PostId),
    Proposal(u32),
    Feature,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Post {
    pub id: PostId,
    pub body: String,
    pub user: UserId,
    #[serde(default)]
    pub timestamp: u64,
    #[serde(default)]
    pub children: Vec<PostId>,
    #[serde(default)]
    pub parent: Option<PostId>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub reactions: BTreeMap<u16, Vec<UserId>>,
    #[serde(default)]
    pub patches: Vec<(u64, String)>,
    #[serde(default)]
    pub files: BTreeMap<String, (u64, u64)>,
    #[serde(default)]
    pub tips: Vec<(UserId, u64)>,
    #[serde(default)]
    pub extension: Option<Extension>,
    #[serde(default)]
    pub realm: Option<String>,
    #[serde(default)]
    pub hashes: Vec<String>,
    #[serde(default)]
    pub reposts: Vec<PostId>,
    #[serde(default)]
    pub encrypted: bool,
    #[serde(default)]
    pub hidden_for: Vec<UserId>,
}

impl Post {
    pub fn reaction_count(&self, code: u16) -> usize {
        self.reactions.get(&code).map(Vec::len).unwrap_or(0)
    }

    pub fn total_reactions(&self) -> usize {
        self.reactions.values().map(Vec::len).sum()
    }

    pub fn is_reply(&self) -> bool {
        self.parent.is_some()
    }

    pub fn age_secs(&self, now_ns: u64) -> u64 {
        now_ns.saturating_sub(self.timestamp) / 1_000_000_000
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub author_name: String,
    #[serde(default)]
    pub author_filters: Value,
    #[serde(default)]
    pub viewer_blocked: bool,
    #[serde(default)]
    pub realm_color: Option<String>,
    #[serde(default)]
    pub nsfw: bool,
    #[serde(default)]
    pub max_downvotes_reached: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub id: UserId,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub account: String,
    #[serde(default)]
    pub principal: String,
    #[serde(default)]
    pub bucket: Option<String>,
    #[serde(default)]
    pub bookmarks: Vec<PostId>,
    #[serde(default)]
    pub pinned_posts: Vec<PostId>,
    #[serde(default)]
    pub followees: Vec<UserId>,
    #[serde(default)]
    pub followers: Vec<UserId>,
    #[serde(default)]
    pub realms: Vec<String>,
    #[serde(default)]
    pub balance: u64,
    #[serde(default)]
    pub cold_balance: u64,
    #[serde(default)]
    pub last_activity: u64,
    #[serde(default)]
    pub stalwart: bool,
    #[serde(default)]
    pub governance: bool,
    #[serde(default)]
    pub num_posts: usize,
    #[serde(default)]
    pub settings: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResult {
    pub id: PostId,
    pub user_id: UserId,
    #[serde(default)]
    pub generic_id: String,
    #[serde(default)]
    pub result: String,
    #[serde(default)]
    pub relevant: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Stats {
    #[serde(default)]
    pub users: u64,
    #[serde(default)]
    pub posts: u64,
    #[serde(default)]
    pub e8s_for_one_xdr: u64,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// A post together with its viewer-dependent metadata, as returned by
/// `with_meta` endpoints.
#[derive(Debug, Clone)]
pub struct PostItem {
    pub post: Post,
    pub author: String,
    pub nsfw: bool,
    pub realm_color: Option<String>,
    pub viewer_blocked: bool,
}

impl PostItem {
    pub fn from_pair(pair: (Post, Meta)) -> Self {
        let (post, meta) = pair;
        Self {
            post,
            author: meta.author_name,
            nsfw: meta.nsfw,
            realm_color: meta.realm_color,
            viewer_blocked: meta.viewer_blocked,
        }
    }

    pub fn id(&self) -> PostId {
        self.post.id
    }
}

pub fn reaction_set() -> Vec<(u16, &'static str)> {
    vec![
        (10, "❤"),
        (11, "👍"),
        (12, "😢"),
        (50, "🔥"),
        (51, "😂"),
        (52, "💯"),
        (53, "🚀"),
        (100, "⭐"),
        (101, "🏴"),
        (1, "❌"),
    ]
}

pub fn reaction_icon(code: u16) -> &'static str {
    reaction_set()
        .into_iter()
        .find(|(c, _)| *c == code)
        .map(|(_, i)| i)
        .unwrap_or("?")
}

pub type UserNames = HashMap<UserId, String>;
pub type Reactions = BTreeMap<u16, BTreeSet<UserId>>;
