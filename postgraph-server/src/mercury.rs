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

#[derive(Debug, Deserialize)]
pub struct AnalyzedPost {
    pub post_id: String,
    pub intent: String,
    pub subject: String,
    pub sentiment: f32,
}

#[derive(Debug, Deserialize)]
struct AnalysisResponse {
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
        posts: &[(String, String)],
        existing_intents: &[String],
        existing_subjects: &[String],
    ) -> Result<Vec<AnalyzedPost>, AppError> {
        let intents_list = if existing_intents.is_empty() {
            "None yet".to_string()
        } else {
            existing_intents.join(", ")
        };

        let subjects_list = if existing_subjects.is_empty() {
            "None yet".to_string()
        } else {
            existing_subjects.join(", ")
        };

        let posts_json: Vec<serde_json::Value> = posts
            .iter()
            .map(|(id, text)| serde_json::json!({"id": id, "text": text}))
            .collect();
        let posts_json_str = serde_json::to_string_pretty(&posts_json).unwrap_or_default();

        let prompt = format!(
            r#"You are analyzing social media posts for a content analytics platform.

For each post, extract:
1. **Intent** — what the post is trying to do (one per post)
2. **Subject** — what the post is about (one per post)
3. **Sentiment** — emotional tone (-1.0 to 1.0)

## Intent (pick exactly one)
The communicative purpose of the post. Seed examples:
- Question: asking the audience something
- Hot take: strong opinion meant to provoke thought
- Humor: joke, wordplay, absurdist observation
- Story: personal anecdote or experience
- Tip: sharing something useful or instructional
- Hype: excitement, celebrating a win or milestone
- Rant: frustration, complaint, venting
- Observation: noticing something interesting, neutral tone
- Promotion: sharing own work, project, or product

You may create new intents if a post genuinely doesn't fit any of these, but apply the reusability test first.

## Subject (pick exactly one)
The topic domain of the post. Seed examples:
- AI & LLMs, Software dev, Side projects, Social media, Productivity, Daily life, Gaming, Career, Health, Culture, Tech industry, Politics

You may create new subjects at this same granularity level.

## Rules
1. REUSABILITY TEST: Before creating a new intent or subject, ask: "Would this apply to at least 10 posts from a typical creator?" If no, use a broader existing tag.
2. NO COMPOUND TAGS: "Coffee humor" is wrong. That's intent=Humor, subject=Daily life.
3. PREFER EXISTING: Always reuse an existing intent/subject before creating a new one.
4. SHORT NAMES: Max 3 words per tag.
5. NEVER describe a single post's specific content as a tag. "UNO house rules" → subject=Gaming, intent=Question. "Parking preference" → subject=Daily life, intent=Question.

Existing intents: {intents_list}
Existing subjects: {subjects_list}

Posts: {posts_json_str}

Respond with ONLY valid JSON:
{{"posts": [{{"post_id": "...", "intent": "...", "subject": "...", "sentiment": 0.5}}]}}"#
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

        Ok(analysis.posts)
    }
}
