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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryGroup {
    pub name: String,
    pub description: String,
    pub topics: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CategorizeResponse {
    pub categories: Vec<CategoryGroup>,
}

#[derive(Debug, Deserialize)]
pub struct AssignCategoryResponse {
    pub category: String,
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

    /// Lightweight connectivity check — lists available models.
    pub async fn health_check(&self) -> Result<(), AppError> {
        let resp = self
            .client
            .get(format!("{}/models", self.api_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::MercuryApi(body));
        }
        Ok(())
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

    pub async fn categorize_topics(
        &self,
        topics: &[(String, String)], // (name, description)
    ) -> Result<CategorizeResponse, AppError> {
        let topics_json: Vec<serde_json::Value> = topics
            .iter()
            .map(|(name, desc)| serde_json::json!({"name": name, "description": desc}))
            .collect();
        let topics_json_pretty = serde_json::to_string_pretty(&topics_json).unwrap_or_default();

        let prompt = format!(
            r#"Group these topics into broad categories. Each topic should belong to exactly one category. Let the number of categories emerge naturally from the data.

Topics:
{topics_json_pretty}

Respond with ONLY valid JSON in this exact format:
{{
  "categories": [
    {{
      "name": "Category Name",
      "description": "Brief description of what this category covers",
      "topics": ["Topic A", "Topic B"]
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

        let categorize: CategorizeResponse = serde_json::from_str(json_str).map_err(|e| {
            AppError::MercuryApi(format!("Failed to parse response: {e}. Raw: {json_str}"))
        })?;

        Ok(categorize)
    }

    pub async fn assign_topic_category(
        &self,
        topic_name: &str,
        categories: &[(String, String)], // (name, description)
    ) -> Result<AssignCategoryResponse, AppError> {
        let cats_json = serde_json::to_string(
            &categories
                .iter()
                .map(|(name, desc)| serde_json::json!({"name": name, "description": desc}))
                .collect::<Vec<_>>(),
        )
        .unwrap_or_default();

        let prompt = format!(
            r#"Given the topic "{topic_name}" and these existing categories: {cats_json}, which category does this topic belong to? Return ONLY valid JSON: {{"category": "<category_name>"}}"#
        );

        let request = ChatRequest {
            model: "mercury-2".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt,
            }],
            temperature: 0.1,
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

        let assign: AssignCategoryResponse = serde_json::from_str(json_str).map_err(|e| {
            AppError::MercuryApi(format!("Failed to parse response: {e}. Raw: {json_str}"))
        })?;

        Ok(assign)
    }
}
