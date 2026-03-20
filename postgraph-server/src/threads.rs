use crate::error::AppError;
use chrono::{DateTime, Utc};
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

#[derive(Debug, Default)]
pub struct PostInsights {
    pub views: i32,
    pub likes: i32,
    pub replies: i32,
    pub reposts: i32,
    pub quotes: i32,
    pub shares: i32,
}

/// Daily view count from the user-level insights endpoint.
#[derive(Debug)]
pub struct UserDailyViews {
    pub date: DateTime<Utc>,
    pub views: i64,
}

/// Aggregated user-level insights covering multiple 90-day windows.
#[derive(Debug, Default)]
pub struct UserInsights {
    pub total_views: i64,
    pub daily: Vec<UserDailyViews>,
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

    /// Fetch user-level insights (views) from the Threads API.
    /// The API limits queries to 90-day windows, so we paginate backwards
    /// to collect up to `max_days` of history (default 730 = ~2 years).
    pub async fn get_user_insights(&self, max_days: Option<u32>) -> Result<UserInsights, AppError> {
        let max_days = max_days.unwrap_or(730) as i64;
        let mut result = UserInsights::default();
        let now = Utc::now();
        let earliest = now - chrono::Duration::days(max_days);

        // Walk backwards in 90-day windows
        let mut window_end = now;
        while window_end > earliest {
            let window_start = (window_end - chrono::Duration::days(89)).max(earliest);
            let since = window_start.timestamp();
            let until = window_end.timestamp();

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
                        // end_time marks the end of the period
                        // We don't have end_time in InsightValue, so attribute to window
                        result.total_views += count;
                        result.daily.push(UserDailyViews {
                            date: window_start.with_timezone(&Utc),
                            views: count,
                        });
                    }
                }
                if let Some(tv) = &item.total_value {
                    let count = tv
                        .as_i64()
                        .or_else(|| tv.get("value").and_then(|v| v.as_i64()))
                        .unwrap_or(0);
                    if result.daily.is_empty() {
                        // total_value without daily breakdown
                        result.total_views += count;
                    }
                }
            }

            // Move window back
            window_end = window_start - chrono::Duration::days(1);
            // Small delay to avoid rate limiting
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }

        Ok(result)
    }
}
