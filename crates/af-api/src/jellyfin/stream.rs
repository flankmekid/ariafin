use af_core::types::{AuthToken, CoverArtId, LyricsData, StreamUrl, TrackId};
use crate::error::ApiError;
use super::JellyfinClient;

impl JellyfinClient {
    pub(crate) fn build_stream_url(
        &self,
        token: &AuthToken,
        id: &TrackId,
        max_bitrate_kbps: Option<u32>,
    ) -> StreamUrl {
        let bitrate = max_bitrate_kbps
            .map(|k| k * 1000)
            .unwrap_or(140_000_000); // 140 Mbps = essentially unlimited

        StreamUrl(format!(
            "{}/Audio/{}/universal\
             ?UserId={}&api_key={}\
             &MaxStreamingBitrate={}\
             &Container=opus,mp3,aac,m4a,flac,webma,webm,wav,ogg",
            self.base_url,
            id.0,
            token.user_id,
            token.token,
            bitrate,
        ))
    }

    pub(crate) fn build_cover_art_url(
        &self,
        token: &AuthToken,
        id: &CoverArtId,
        size: u32,
    ) -> String {
        format!(
            "{}/Items/{}/Images/Primary\
             ?width={}&quality=90&api_key={}",
            self.base_url, id.0, size, token.token,
        )
    }

    pub(crate) async fn fetch_lyrics(
        &self,
        token: &AuthToken,
        id: &TrackId,
    ) -> Result<Option<LyricsData>, ApiError> {
        let url = format!(
            "{}/Audio/{}/Lyrics",
            self.base_url, id.0,
        );

        let resp = self
            .client
            .get(&url)
            .header("X-Emby-Authorization", self.auth_header(Some(&token.token)))
            .send()
            .await?;

        if resp.status() == 404 {
            return Ok(None);
        }
        self.check_status(&resp)?;

        // Jellyfin returns { "Lyrics": [{ "Start": <ticks>, "Text": "..." }] }
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ApiError::Parse(e.to_string()))?;

        let lines = json["Lyrics"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|v| af_core::types::LyricsLine {
                        timestamp_ms: v["Start"]
                            .as_i64()
                            .map(|t| (t / 10_000) as u32), // ticks → ms
                        text: v["Text"]
                            .as_str()
                            .unwrap_or("")
                            .to_string(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if lines.is_empty() {
            return Ok(None);
        }

        let synced = lines.iter().any(|l| l.timestamp_ms.is_some());
        Ok(Some(LyricsData { synced, lines }))
    }

    pub(crate) async fn report_start(
        &self, token: &AuthToken, id: &TrackId,
    ) -> Result<(), ApiError> {
        let url = format!("{}/Sessions/Playing", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("X-Emby-Authorization", self.auth_header(Some(&token.token)))
            .json(&serde_json::json!({ "ItemId": id.0 }))
            .send()
            .await?;
        self.check_status(&resp)?;
        Ok(())
    }

    pub(crate) async fn report_stop(
        &self, token: &AuthToken, id: &TrackId, position_secs: f64,
    ) -> Result<(), ApiError> {
        let url = format!("{}/Sessions/Playing/Stopped", self.base_url);
        let ticks = (position_secs * 10_000_000.0) as i64;
        let resp = self
            .client
            .post(&url)
            .header("X-Emby-Authorization", self.auth_header(Some(&token.token)))
            .json(&serde_json::json!({
                "ItemId": id.0,
                "PositionTicks": ticks,
            }))
            .send()
            .await?;
        self.check_status(&resp)?;
        Ok(())
    }
}
