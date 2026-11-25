use anyhow::{Result, anyhow};
use candid::{CandidType, Decode, Deserialize as CandidDeserialize, Encode};
use serde_json::{Value, json};

use super::Client;

#[derive(CandidType, CandidDeserialize, Debug)]
enum AddPostResult {
    #[serde(rename = "Ok")]
    Ok(u64),
    #[serde(rename = "Err")]
    Err(String),
}

fn check_ok(value: &Value) -> Result<()> {
    if let Some(err) = value.get("Err") {
        return Err(anyhow!(err.as_str().unwrap_or("unknown error").to_string()));
    }
    Ok(())
}

impl Client {
    pub async fn react(&self, post_id: u64, code: u16) -> Result<()> {
        let value = self
            .ic
            .update_json_value("react", vec![json!(post_id), json!(code)])
            .await?;
        check_ok(&value)
    }

    pub async fn toggle_bookmark(&self, post_id: u64) -> Result<bool> {
        self.ic
            .update_json("toggle_bookmark", vec![json!(post_id)])
            .await
    }

    pub async fn toggle_following_post(&self, post_id: u64) -> Result<bool> {
        self.ic
            .update_json("toggle_following_post", vec![json!(post_id)])
            .await
    }

    pub async fn toggle_hide_post(&self, post_id: u64) -> Result<bool> {
        self.ic
            .update_json("toggle_hide_post", vec![json!(post_id)])
            .await
    }

    pub async fn toggle_pinned_post(&self, post_id: u64) -> Result<()> {
        let value = self
            .ic
            .update_json_value("toggle_pinned_post", vec![json!(post_id)])
            .await?;
        check_ok(&value)
    }

    pub async fn vote_on_poll(&self, post_id: u64, option: u16, anonymously: bool) -> Result<()> {
        let value = self
            .ic
            .update_json_value(
                "vote_on_poll",
                vec![json!(post_id), json!(option), json!(anonymously)],
            )
            .await?;
        check_ok(&value)
    }

    pub async fn report(&self, post_id: u64, reason: &str) -> Result<()> {
        let value = self
            .ic
            .update_json_value("report", vec![json!(post_id), json!(reason)])
            .await?;
        check_ok(&value)
    }

    pub async fn vote_on_report(&self, report_id: u64, vote: bool) -> Result<()> {
        let value = self
            .ic
            .update_json_value("vote_on_report", vec![json!(report_id), json!(vote)])
            .await?;
        check_ok(&value)
    }

    pub async fn tip(&self, post_id: u64, amount: u64) -> Result<()> {
        let value = self
            .ic
            .update_json_value("tip", vec![json!(post_id), json!(amount)])
            .await?;
        check_ok(&value)
    }

    pub async fn add_post(
        &self,
        body: &str,
        refs: Vec<(String, u64, u64)>,
        parent: Option<u64>,
        realm: Option<String>,
        extension: Option<Value>,
    ) -> Result<u64> {
        let extension_blob: Option<Vec<u8>> = match extension {
            Some(v) => Some(crate::ic::json_blob(&v)?),
            None => None,
        };
        let arg = Encode!(&body, &refs, &parent, &realm, &extension_blob)
            .map_err(|e| anyhow!("encoding add_post: {e}"))?;
        let reply = self.ic.update_raw("add_post", arg).await?;
        let result: AddPostResult =
            Decode!(&reply, AddPostResult).map_err(|e| anyhow!("decoding add_post: {e}"))?;
        match result {
            AddPostResult::Ok(id) => Ok(id),
            AddPostResult::Err(e) => Err(anyhow!(e)),
        }
    }

    pub async fn create_repost(
        &self,
        post_id: u64,
        body: &str,
        realm: Option<String>,
    ) -> Result<u64> {
        self.add_post(
            body,
            vec![],
            None,
            realm,
            Some(json!({ "Repost": post_id })),
        )
        .await
    }

    pub async fn create_poll(
        &self,
        body: &str,
        options: Vec<String>,
        deadline_hours: u64,
        realm: Option<String>,
    ) -> Result<u64> {
        let poll = json!({
            "options": options,
            "votes": {},
            "voters": [],
            "deadline": deadline_hours,
            "weighted_by_karma": {},
            "weighted_by_tokens": {},
        });
        self.add_post(body, vec![], None, realm, Some(json!({ "Poll": poll })))
            .await
    }
}
