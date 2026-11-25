use anyhow::Result;
use serde_json::{Value, json};

use super::models::{Post, PostItem, SearchResult, Stats, User, UserId, UserNames};
use super::{Client, now_ns};

impl Client {
    async fn feed_pairs(&self, method: &str, args: Vec<Value>) -> Result<Vec<PostItem>> {
        let pairs: Vec<(Post, super::models::Meta)> = self.ic.query_json(method, args).await?;
        Ok(pairs.into_iter().map(PostItem::from_pair).collect())
    }

    pub async fn latest_posts(
        &self,
        realm: &str,
        page: usize,
        offset: u64,
    ) -> Result<Vec<PostItem>> {
        self.feed_pairs(
            "last_posts",
            vec![
                json!(self.domain),
                json!(realm),
                json!(page),
                json!(offset),
                json!(false),
            ],
        )
        .await
    }

    pub async fn hot_posts(&self, realm: &str, page: usize, offset: u64) -> Result<Vec<PostItem>> {
        self.feed_pairs(
            "hot_posts",
            vec![
                json!(self.domain),
                json!(realm),
                json!(page),
                json!(offset),
                json!(false),
            ],
        )
        .await
    }

    pub async fn personal_feed(&self, page: usize, offset: u64) -> Result<Vec<PostItem>> {
        self.feed_pairs(
            "personal_feed",
            vec![json!(self.domain), json!(page), json!(offset)],
        )
        .await
    }

    pub async fn thread(&self, id: u64) -> Result<Vec<PostItem>> {
        self.feed_pairs("thread", vec![json!(id)]).await
    }

    pub async fn posts(&self, ids: Vec<u64>) -> Result<Vec<PostItem>> {
        self.feed_pairs("posts", vec![json!(ids)]).await
    }

    pub async fn user_posts(
        &self,
        handle: &str,
        page: usize,
        offset: u64,
    ) -> Result<Vec<PostItem>> {
        self.feed_pairs(
            "user_posts",
            vec![
                json!(self.domain),
                json!(handle),
                json!(page),
                json!(offset),
            ],
        )
        .await
    }

    pub async fn journal(&self, handle: &str, page: usize, offset: u64) -> Result<Vec<PostItem>> {
        self.feed_pairs(
            "journal",
            vec![
                json!(self.domain),
                json!(handle),
                json!(page),
                json!(offset),
            ],
        )
        .await
    }

    /// Fetch a profile. `handle == ""` and empty `handles` means "my profile".
    pub async fn user(&self, handle: Option<&str>) -> Result<Option<User>> {
        let handles: Vec<String> = handle.map(|h| vec![h.to_string()]).unwrap_or_default();
        let value = self
            .ic
            .query_json_value("user", vec![json!(self.domain), json!(handles)])
            .await?;
        Ok(serde_json::from_value(value)?)
    }

    pub async fn users_data(&self, ids: Vec<UserId>) -> Result<UserNames> {
        self.ic.query_json("users_data", vec![json!(ids)]).await
    }

    pub async fn search(&self, query: &str) -> Result<Vec<SearchResult>> {
        self.ic
            .query_json("search", vec![json!(self.domain), json!(query)])
            .await
    }

    pub async fn stats(&self) -> Result<Stats> {
        self.ic.query_json("stats", vec![]).await
    }

    pub fn now(&self) -> u64 {
        now_ns()
    }
}
