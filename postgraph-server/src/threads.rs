use crate::error::AppError;
use reqwest::Client;
use serde::Deserialize;

const BASE_URL: &str = "https://graph.threads.net/v1.0";

pub struct ThreadsClient {
    client: Client,
    access_token: String,
}

#[derive(Debug, Deserialize)]
pub struct ThreadsPost {
    pub id: String,
    pub text: Option<String>,
    pub media_type: Option<String>,
    pub media_url: Option<String>,
    pub timestamp: Option<String>,
    pub permalink: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ThreadsPaging {
    pub cursors: Option<ThreadsCursors>,
    pub next: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ThreadsCursors {
    pub after: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ThreadsListResponse {
    pub data: Vec<ThreadsPost>,
    pub paging: Option<ThreadsPaging>,
}

#[derive(Debug, Deserialize)]
pub struct InsightValue {
    pub value: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct InsightData {
    pub name: String,
    pub values: Option<Vec<InsightValue>>,
    // Some metrics return total_value instead of values
    pub total_value: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct InsightsResponse {
    pub data: Vec<InsightData>,
}

#[derive(Debug, Default)]
pub struct PostInsights {
    pub views: i32,
    pub likes: i32,
    pub replies: i32,
    pub reposts: i32,
    pub quotes: i32,
    pub shares: i32,
}

impl ThreadsClient {
    pub fn new(access_token: String) -> Self {
        Self {
            client: Client::new(),
            access_token,
        }
    }

    pub async fn get_user_threads(
        &self,
        cursor: Option<&str>,
    ) -> Result<ThreadsListResponse, AppError> {
        let mut url = format!(
            "{}/me/threads?fields=id,text,media_type,media_url,timestamp,permalink&access_token={}",
            BASE_URL, self.access_token
        );
        if let Some(c) = cursor {
            url.push_str(&format!("&after={}", c));
        }

        let resp = self.client.get(&url).send().await?;
        if resp.status() == 429 {
            return Err(AppError::RateLimited(60));
        }
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::ThreadsApi(body));
        }
        let data: ThreadsListResponse = resp.json().await?;
        Ok(data)
    }

    pub async fn get_post_insights(&self, post_id: &str) -> Result<PostInsights, AppError> {
        let url = format!(
            "{}/{}/insights?metric=views,likes,replies,reposts,quotes,shares&access_token={}",
            BASE_URL, post_id, self.access_token
        );

        let resp = self.client.get(&url).send().await?;
        if resp.status() == 429 {
            return Err(AppError::RateLimited(60));
        }
        if !resp.status().is_success() {
            // Some posts may not support insights; return zeros
            return Ok(PostInsights::default());
        }

        let data: InsightsResponse = resp.json().await?;
        let mut insights = PostInsights::default();

        for item in &data.data {
            let value = item
                .total_value
                .as_ref()
                .and_then(|v| {
                    // Handle both bare integer and {"value": N} object formats
                    v.as_i64()
                        .or_else(|| v.get("value").and_then(|inner| inner.as_i64()))
                })
                .or_else(|| {
                    item.values
                        .as_ref()
                        .and_then(|vals| vals.first())
                        .and_then(|v| v.value.as_ref())
                        .and_then(|v| v.as_i64())
                })
                .unwrap_or(0) as i32;

            match item.name.as_str() {
                "views" => insights.views = value,
                "likes" => insights.likes = value,
                "replies" => insights.replies = value,
                "reposts" => insights.reposts = value,
                "quotes" => insights.quotes = value,
                "shares" => insights.shares = value,
                _ => {}
            }
        }

        Ok(insights)
    }
}
