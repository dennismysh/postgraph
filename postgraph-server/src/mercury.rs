use crate::error::AppError;
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct MercuryClient {
    client: Client,
    api_key: String,
    api_url: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ChatResponseMessage {
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzedPost {
    pub post_id: String,
    pub topics: Vec<TopicAssignment>,
    pub sentiment: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicAssignment {
    pub name: String,
    pub description: String,
    pub weight: f32,
}

#[derive(Debug, Deserialize)]
pub struct AnalysisResponse {
    pub posts: Vec<AnalyzedPost>,
}

impl MercuryClient {
    pub fn new(api_key: String, api_url: String) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("failed to build HTTP client");
        Self {
            client,
            api_key,
            api_url,
        }
    }

    pub async fn analyze_posts(
        &self,
        posts: &[(String, String)], // (id, text)
        existing_topics: &[String],
    ) -> Result<AnalysisResponse, AppError> {
        let topics_list = if existing_topics.is_empty() {
            "No existing topics yet.".to_string()
        } else {
            existing_topics.join(", ")
        };

        let posts_json: Vec<serde_json::Value> = posts
            .iter()
            .map(|(id, text)| serde_json::json!({"id": id, "text": text}))
            .collect();
        let posts_json_str = serde_json::to_string_pretty(&posts_json).unwrap_or_default();

        let prompt = format!(
            r#"Analyze these social media posts. For each post, extract:
1. Topics (map to existing topics when possible, create new ones only when needed)
2. Sentiment (-1.0 to 1.0)

Existing topics: [{topics_list}]

Posts:
{posts_json_str}

Respond with ONLY valid JSON in this exact format:
{{
  "posts": [
    {{
      "post_id": "the id",
      "topics": [
        {{"name": "Topic Name", "description": "Brief description", "weight": 0.8}}
      ],
      "sentiment": 0.5
    }}
  ]
}}"#
        );

        let request = ChatRequest {
            model: "mercury-2".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt,
            }],
            temperature: 0.3,
        };

        let resp = self
            .client
            .post(format!("{}/chat/completions", self.api_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::MercuryApi(body));
        }

        let chat_resp: ChatResponse = resp.json().await?;
        let content = chat_resp
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        // Parse JSON from response, stripping markdown code fences if present
        let json_str = content
            .trim()
            .strip_prefix("```json")
            .or_else(|| content.trim().strip_prefix("```"))
            .unwrap_or(content.trim())
            .strip_suffix("```")
            .unwrap_or(content.trim())
            .trim();

        let analysis: AnalysisResponse = serde_json::from_str(json_str).map_err(|e| {
            AppError::MercuryApi(format!("Failed to parse response: {e}. Raw: {json_str}"))
        })?;

        Ok(analysis)
    }
}
