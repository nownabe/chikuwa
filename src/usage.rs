use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct Usage {
    pub five_hour: f64,
    pub seven_day: f64,
}

#[derive(Deserialize)]
struct ApiResponse {
    five_hour: UtilizationBucket,
    seven_day: UtilizationBucket,
}

#[derive(Deserialize)]
struct UtilizationBucket {
    utilization: f64,
}

#[derive(Deserialize)]
struct Credentials {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<OAuthEntry>,
}

#[derive(Deserialize)]
struct OAuthEntry {
    #[serde(rename = "accessToken")]
    access_token: String,
}

fn read_access_token() -> Result<String> {
    let home = std::env::var("HOME").context("HOME not set")?;
    let path = std::path::PathBuf::from(home)
        .join(".claude")
        .join(".credentials.json");
    let data = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let creds: Credentials =
        serde_json::from_str(&data).context("Failed to parse credentials JSON")?;
    creds
        .claude_ai_oauth
        .map(|o| o.access_token)
        .context("No claudeAiOauth.accessToken in credentials")
}

/// Result of a usage fetch attempt.
#[derive(Debug)]
pub enum FetchResult {
    Success(Usage),
    RateLimited,
    Error(anyhow::Error),
}

pub async fn fetch_usage() -> FetchResult {
    let token = match read_access_token() {
        Ok(t) => t,
        Err(e) => return FetchResult::Error(e),
    };
    let client = reqwest::Client::new();
    let resp = match client
        .get("https://api.anthropic.com/api/oauth/usage")
        .bearer_auth(&token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return FetchResult::Error(anyhow::anyhow!("Failed to send usage request: {e}")),
    };
    if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return FetchResult::RateLimited;
    }
    let resp = match resp.error_for_status() {
        Ok(r) => r,
        Err(e) => return FetchResult::Error(anyhow::anyhow!("Usage API returned error: {e}")),
    };
    match resp.json::<ApiResponse>().await {
        Ok(api) => FetchResult::Success(Usage {
            five_hour: api.five_hour.utilization / 100.0,
            seven_day: api.seven_day.utilization / 100.0,
        }),
        Err(e) => FetchResult::Error(anyhow::anyhow!("Failed to parse usage response: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_api_response() {
        let json = r#"{"five_hour":{"utilization":63.0,"resets_at":"2026-03-09T16:00:00+00:00"},"seven_day":{"utilization":19.0,"resets_at":"2026-03-14T09:00:00+00:00"}}"#;
        let api: ApiResponse = serde_json::from_str(json).unwrap();
        assert!((api.five_hour.utilization - 63.0).abs() < f64::EPSILON);
        assert!((api.seven_day.utilization - 19.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_credentials() {
        let json = r#"{"claudeAiOauth":{"accessToken":"tok_123"}}"#;
        let creds: Credentials = serde_json::from_str(json).unwrap();
        assert_eq!(creds.claude_ai_oauth.unwrap().access_token, "tok_123");
    }

    #[test]
    fn test_parse_credentials_missing_oauth() {
        let json = r#"{}"#;
        let creds: Credentials = serde_json::from_str(json).unwrap();
        assert!(creds.claude_ai_oauth.is_none());
    }

    #[test]
    fn test_usage_clone() {
        let usage = Usage {
            five_hour: 0.5,
            seven_day: 0.8,
        };
        let cloned = usage.clone();
        assert!((cloned.five_hour - 0.5).abs() < f64::EPSILON);
        assert!((cloned.seven_day - 0.8).abs() < f64::EPSILON);
    }
}
