use crate::emotions::{EmotionNarrative, EmotionsSummary};
use crate::error::AppError;
use crate::insights::{InsightsContext, InsightsReport};
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
    pub emotion: String,
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
4. **Emotion** — the dominant emotional quality of the post (one per post)

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

## Emotion (pick exactly one)
The dominant emotional quality of the post. Pick from this fixed list only:
- Vulnerable: openness, personal sharing, admitting uncertainty
- Curious: questions, exploration, wonder
- Playful: humor, wit, lightheartedness
- Confident: strong opinions, assertions, expertise
- Reflective: introspection, lessons learned, looking back
- Frustrated: venting, complaints, friction
- Provocative: hot takes, challenging norms, debate-starting

Always pick exactly one from this list. Do not create new emotions.

## Rules
1. REUSABILITY TEST: Before creating a new intent or subject, ask: "Would this apply to at least 10 posts from a typical creator?" If no, use a broader existing tag.
2. NO COMPOUND TAGS: "Coffee humor" is wrong. That's intent=Humor, subject=Daily life.
3. PREFER EXISTING: Always reuse an existing intent/subject before creating a new one.
4. SHORT NAMES: Max 3 words per tag.
5. NEVER describe a single post's specific content as a tag. "UNO house rules" → subject=Gaming, intent=Question. "Parking preference" → subject=Daily life, intent=Question.
6. EMOTION IS FIXED: Only use one of the 7 listed emotions. Never invent new ones.

Existing intents: {intents_list}
Existing subjects: {subjects_list}

Posts: {posts_json_str}

Respond with ONLY valid JSON:
{{"posts": [{{"post_id": "...", "intent": "...", "subject": "...", "sentiment": 0.5, "emotion": "curious"}}]}}"#
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

    pub async fn generate_insights(
        &self,
        context: &InsightsContext,
    ) -> Result<InsightsReport, AppError> {
        let context_json = serde_json::to_string_pretty(context)?;

        let system_prompt = r#"You are a candid friend who happens to be a world-class content strategist. You've just reviewed 30 days of someone's Threads posts along with their engagement and view data. You care about this person's growth and you're not going to sugarcoat things — but you're also not harsh. You're direct, specific, and grounded in the numbers.

Respond with ONLY valid JSON matching this exact structure:
{
  "headline": "<one punchy sentence that captures the most important thing to know right now>",
  "sections": [
    {
      "key": "working",
      "title": "What's Working",
      "summary": "<2-3 sentences on patterns driving results>",
      "items": [
        {
          "observation": "<specific, data-grounded observation>",
          "cited_posts": ["<post id>", ...],
          "tone": "positive"
        }
      ]
    },
    {
      "key": "not_working",
      "title": "What's Not Working",
      "summary": "<2-3 sentences on patterns that are underperforming>",
      "items": [
        {
          "observation": "<specific, data-grounded observation>",
          "cited_posts": ["<post id>", ...],
          "tone": "negative"
        }
      ]
    },
    {
      "key": "on_brand",
      "title": "On Brand",
      "summary": "<2-3 sentences on consistent voice and topics>",
      "items": [
        {
          "observation": "<specific observation about voice, subject matter, or consistency>",
          "cited_posts": ["<post id>", ...],
          "tone": "neutral"
        }
      ]
    },
    {
      "key": "off_pattern",
      "title": "Off Pattern",
      "summary": "<2-3 sentences on anomalies, experiments, or departures from the norm>",
      "items": [
        {
          "observation": "<specific observation about outliers or experiments>",
          "cited_posts": ["<post id>", ...],
          "tone": "neutral"
        }
      ]
    }
  ]
}

Rules:
- Each section must have 2-4 items.
- cited_posts should reference actual post IDs from the provided context.
- Be specific: cite numbers, subjects, intents. Avoid vague statements like "engagement is good".
- The headline should be something a person would actually say out loud to a friend, not a corporate summary.
- Do not wrap JSON in markdown code fences."#;

        let request = ChatRequest {
            model: "mercury-2".to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: format!(
                        "Here is the analytics data for the last 30 days:\n\n{context_json}"
                    ),
                },
            ],
            temperature: 0.5,
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

        // Strip markdown code fences if present
        let json_str = content
            .trim()
            .strip_prefix("```json")
            .or_else(|| content.trim().strip_prefix("```"))
            .unwrap_or(content.trim())
            .strip_suffix("```")
            .unwrap_or(content.trim())
            .trim();

        let report: InsightsReport = serde_json::from_str(json_str).map_err(|e| {
            AppError::MercuryApi(format!(
                "Failed to parse insights response: {e}. Raw: {json_str}"
            ))
        })?;

        Ok(report)
    }

    pub async fn generate_emotion_narrative(
        &self,
        summary: &EmotionsSummary,
    ) -> Result<EmotionNarrative, AppError> {
        let context_json = serde_json::to_string_pretty(summary)?;

        let system_prompt = r#"You are a candid friend who is also a world-class content strategist. You've just reviewed 30 days of someone's social media posts, classified by emotional tone, alongside their engagement data (views, likes, replies, reposts). You care about this person's growth and you're direct, specific, and grounded in the numbers.

Respond with ONLY valid JSON matching this exact structure:
{
  "headline": "<one punchy sentence capturing the most important emotion-engagement insight>",
  "observations": [
    {
      "text": "<specific, data-grounded observation about how an emotion correlates with audience response>",
      "cited_posts": ["<post id>", ...],
      "emotion": "<emotion name>"
    }
  ]
}

Rules:
- Return exactly 3-5 observations.
- Focus on the creator-audience relationship: which emotions resonate, which fall flat, which get reach but not engagement (or vice versa).
- Compare emotions against each other: "Your curious posts get 2x the views of your confident posts."
- Comment on emotional range: is the creator one-note or diverse? Is that helping or hurting?
- cited_posts should reference actual post IDs from the top_post_id fields when available.
- The headline should sound like something a friend would say, not a corporate summary.
- Be specific: cite numbers, percentages, emotion names. Avoid vague statements.
- Do not wrap JSON in markdown code fences."#;

        let request = ChatRequest {
            model: "mercury-2".to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: format!(
                        "Here is the emotion breakdown for the last 30 days:\n\n{context_json}"
                    ),
                },
            ],
            temperature: 0.5,
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

        let json_str = content
            .trim()
            .strip_prefix("```json")
            .or_else(|| content.trim().strip_prefix("```"))
            .unwrap_or(content.trim())
            .strip_suffix("```")
            .unwrap_or(content.trim())
            .trim();

        let narrative: EmotionNarrative = serde_json::from_str(json_str).map_err(|e| {
            AppError::MercuryApi(format!("Failed to parse emotion narrative: {e}. Raw: {json_str}"))
        })?;

        Ok(narrative)
    }
}
