use af_core::types::AuthToken;
use crate::error::ApiError;
use super::{models::AuthResponse, JellyfinClient};

impl JellyfinClient {
    pub async fn do_authenticate(
        &self,
        username: &str,
        password: &str,
    ) -> Result<AuthToken, ApiError> {
        let url = format!("{}/Users/AuthenticateByName", self.base_url);

        let resp = self
            .client
            .post(&url)
            .header("X-Emby-Authorization", self.auth_header(None))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "Username": username,
                "Pw": password,
            }))
            .send()
            .await?;

        let status = resp.status();
        if status == 401 || status == 403 {
            return Err(ApiError::Auth(
                "Invalid username or password".to_string(),
            ));
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ApiError::Http {
                status: status.as_u16(),
                body,
            });
        }

        let auth: AuthResponse = resp
            .json()
            .await
            .map_err(|e| ApiError::Parse(e.to_string()))?;

        Ok(AuthToken {
            token: auth.access_token,
            user_id: auth.user.id,
        })
    }

    pub async fn do_validate_token(&self, token: &AuthToken) -> Result<bool, ApiError> {
        let url = format!("{}/Users/{}", self.base_url, token.user_id);
        let resp = self
            .client
            .get(&url)
            .header("X-Emby-Authorization", self.auth_header(Some(&token.token)))
            .send()
            .await?;

        Ok(resp.status().is_success())
    }
}
