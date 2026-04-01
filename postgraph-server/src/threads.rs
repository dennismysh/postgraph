use crate::error::AppError;
use chrono::Utc;
use reqwest::Client;
use serde::Deserialize;
use tokio::sync::RwLock;

const BASE_URL: &str = "https://graph.threads.net/v1.0";

/// Parse Threads API end_time string (e.g. "2024-07-12T08:00:00+0000") into a NaiveDate.
/// The end_time marks the end of the day period, so we subtract one day to get the actual date.
fn parse_end_time(s: &str) -> Option<chrono::NaiveDate> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some((dt - chrono::Duration::days(1)).date_naive());
    }
    if let Ok(dt) = chrono::DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%z") {
        return Some((dt - chrono::Duration::days(1)).date_naive());
    }
    tracing::warn!("Failed to parse end_time: {s:?}");
    None
}

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
    pub end_time: Option<String>,
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

    /// Fetch user-level daily views from the Threads API.
    /// Returns (date, views) pairs parsed from the API's end_time field.
    /// Walks backwards in 90-day windows up to `max_days` (default 730).
    pub async fn get_user_insights(
        &self,
        max_days: Option<u32>,
    ) -> Result<Vec<(chrono::NaiveDate, i64)>, AppError> {
        let max_days = max_days.unwrap_or(730) as i64;
        let mut result: Vec<(chrono::NaiveDate, i64)> = Vec::new();
        let now = Utc::now();
        let earliest = now - chrono::Duration::days(max_days);

        let mut window_end = now;
        while window_end > earliest {
            let window_start = (window_end - chrono::Duration::days(89)).max(earliest);
            let since = window_start.timestamp();
            let until = window_end.timestamp();

            let url = format!(
                "{}/me/threads_insights?metric=views&since={}&until={}&access_token={}",
                BASE_URL, since, until,
                self.token().await
            );

            let resp = self.client.get(&url).send().await?;
            if resp.status() == 429 {
                return Err(AppError::RateLimited(60));
            }
            if !resp.status().is_success() {
                let body = resp.text().await.unwrap_or_default();
                tracing::warn!("User insights request failed: {body}");
                break;
            }

            let data: InsightsResponse = resp.json().await?;
            for item in &data.data {
                if item.name != "views" {
                    continue;
                }
                if let Some(values) = &item.values {
                    for v in values {
                        let count = v.value.as_ref().and_then(|val| val.as_i64()).unwrap_or(0);
                        if count == 0 {
                            continue;
                        }
                        if let Some(ref end_time) = v.end_time {
                            if let Some(date) = parse_end_time(end_time) {
                                result.push((date, count));
                            }
                        }
                    }
                }
            }

            window_end = window_start - chrono::Duration::days(1);
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }

        Ok(result)
    }
}
