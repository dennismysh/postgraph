use crate::error::AppError;
use reqwest::Client;
use serde::Deserialize;
use tokio::sync::RwLock;

const BASE_URL: &str = "https://graph.threads.net/v1.0";

pub struct ThreadsClient {
    client: Client,
    access_token: RwLock<String>,
}

#[derive(Debug, Deserialize)]
struct RefreshTokenResponse {
    access_token: String,
    expires_in: i64,
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

// User-level insights response types
#[derive(Debug, Deserialize)]
pub struct UserInsightValue {
    pub value: i64,
    pub end_time: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UserInsightData {
    pub name: String,
    pub values: Option<Vec<UserInsightValue>>,
}

#[derive(Debug, Deserialize)]
pub struct UserInsightsResponse {
    pub data: Vec<UserInsightData>,
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
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");
        Self {
            client,
            access_token: RwLock::new(access_token),
        }
    }

    /// Update the in-memory access token (e.g. after loading from DB or refreshing).
    pub async fn set_token(&self, token: String) {
        *self.access_token.write().await = token;
    }

    /// Get a clone of the current access token.
    async fn token(&self) -> String {
        self.access_token.read().await.clone()
    }

    /// Refresh the long-lived token via the Threads API.
    /// Returns the new token and its TTL in seconds.
    pub async fn refresh_token(&self) -> Result<(String, i64), AppError> {
        let current = self.token().await;
        let url = format!(
            "{}/refresh_access_token?grant_type=th_refresh_token&access_token={}",
            BASE_URL, current
        );
        let resp = self.client.get(&url).send().await?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::ThreadsApi(format!(
                "Token refresh failed: {body}"
            )));
        }
        let data: RefreshTokenResponse = resp.json().await?;
        self.set_token(data.access_token.clone()).await;
        Ok((data.access_token, data.expires_in))
    }

    /// Lightweight connectivity check — fetches the authenticated user's profile.
    pub async fn health_check(&self) -> Result<(), AppError> {
        let url = format!(
            "{}/me?fields=id&access_token={}",
            BASE_URL,
            self.token().await
        );
        let resp = self.client.get(&url).send().await?;
        if resp.status() == 429 {
            return Err(AppError::RateLimited(60));
        }
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::ThreadsApi(body));
        }
        Ok(())
    }

    pub async fn get_user_threads(
        &self,
        cursor: Option<&str>,
    ) -> Result<ThreadsListResponse, AppError> {
        let mut url = format!(
            "{}/me/threads?fields=id,text,media_type,media_url,timestamp,permalink&access_token={}",
            BASE_URL,
            self.token().await
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
            BASE_URL,
            post_id,
            self.token().await
        );

        let resp = self.client.get(&url).send().await?;
        if resp.status() == 429 {
            return Err(AppError::RateLimited(60));
        }
        if !resp.status().is_success() {
            // Some posts may not support insights; signal the caller so it
            // can decide whether to overwrite existing metrics.
            return Err(AppError::ThreadsApi(format!(
                "Insights unavailable (HTTP {})",
                resp.status()
            )));
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

    /// Fetch user-level daily views from the Threads Insights API.
    /// Returns daily view counts as (date_string, views) pairs.
    pub async fn get_user_views(
        &self,
        since: i64,
        until: i64,
    ) -> Result<Vec<(String, i64)>, AppError> {
        let url = format!(
            "{}/me/threads_insights?metric=views&since={}&until={}&access_token={}",
            BASE_URL,
            since,
            until,
            self.token().await
        );

        let resp = self.client.get(&url).send().await?;
        if resp.status() == 429 {
            return Err(AppError::RateLimited(60));
        }
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::ThreadsApi(format!(
                "User insights failed: {body}"
            )));
        }

        let data: UserInsightsResponse = resp.json().await?;
        let mut result = Vec::new();

        for item in &data.data {
            if item.name == "views" {
                if let Some(values) = &item.values {
                    for v in values {
                        if let Some(end_time) = &v.end_time {
                            // Parse end_time like "2024-07-12T08:00:00+0000"
                            let date = if let Ok(dt) =
                                chrono::DateTime::parse_from_str(end_time, "%Y-%m-%dT%H:%M:%S%z")
                            {
                                dt.format("%Y-%m-%d").to_string()
                            } else {
                                // Fallback: take first 10 chars as date
                                end_time.chars().take(10).collect()
                            };
                            result.push((date, v.value));
                        }
                    }
                }
            }
        }

        Ok(result)
    }
}
