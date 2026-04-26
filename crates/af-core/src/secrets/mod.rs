use anyhow::Result;
use keyring::Entry;
use tracing::warn;

const SERVICE_NAME: &str = "ariafin";

/// Store credentials for a server URL.
/// Uses the server URL as the keyring entry identifier (format: "ariafin-{server_url}").
pub fn store_credentials(server_url: &str, user_id: &str, token: &str) -> Result<()> {
    let entry = Entry::new(SERVICE_NAME, &format!("ariafin-{}", server_url))?;
    entry.set_password(&format!("{token}:{user_id}"))?;
    Ok(())
}

/// Retrieve credentials for a server URL.
/// Returns (token, user_id).
pub fn get_credentials(server_url: &str) -> Result<(String, String)> {
    let entry = Entry::new(SERVICE_NAME, &format!("ariafin-{}", server_url))?;
    let creds = entry.get_password()?;
    let mut parts = creds.split(':');
    let token = parts.next().ok_or_else(|| anyhow::anyhow!("Invalid credential format"))?;
    let user_id = parts.next().ok_or_else(|| anyhow::anyhow!("Invalid credential format"))?;
    Ok((token.to_string(), user_id.to_string()))
}

/// Delete credentials for a server URL.
pub fn delete_credentials(server_url: &str) -> Result<()> {
    let entry = Entry::new(SERVICE_NAME, &format!("ariafin-{}", server_url))?;
    entry.delete_password()?;
    Ok(())
}

/// Try to get credentials, returning Ok(None) if the keyring is unavailable or entry not found.
pub fn try_get_credentials(server_url: &str) -> Result<Option<(String, String)>> {
    match get_credentials(server_url) {
        Ok(creds) => Ok(Some(creds)),
        Err(e) => {
            // Log warning but don't fail; the system may not have a keyring
            warn!("Failed to retrieve credentials from keyring for {}: {}", server_url, e);
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credential_roundtrip() -> Result<()> {
        let server_url = "http://test.example.com";
        let token = "testtoken";
        let user_id = "testuserid";

        store_credentials(server_url, user_id, token)?;
        let (retrieved_token, retrieved_user_id) = get_credentials(server_url)?;
        assert_eq!(token, retrieved_token);
        assert_eq!(user_id, retrieved_user_id);
        delete_credentials(server_url)?;
        Ok(())
    }
}