//! Lyrics from Spotify's own `color-lyrics` endpoint via librespot (no Web API grant needed).
//!
//! Response shape (verified 2026-10-03): `{"lyrics": {"syncType": "LINE_SYNCED" | "UNSYNCED",
//! "lines": [{"startTimeMs": "18810", "words": "..."}]}}`; times are strings. No lyrics → 404.

use librespot::core::{Error, SpotifyId, error::ErrorKind, session::Session};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lyrics {
    pub lines: Vec<String>,
    /// Start time per line, only when Spotify synced every line.
    pub times_ms: Option<Vec<u32>>,
}

/// `Ok(None)` means the track simply has no lyrics.
pub async fn fetch(session: &Session, track_uri: &str) -> Result<Option<Lyrics>, String> {
    let Some(id) = track_uri.strip_prefix("spotify:track:").and_then(|id| SpotifyId::from_base62(id).ok()) else {
        return Ok(None);
    };
    match session.spclient().get_lyrics(&id).await {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).ok().and_then(|json| parse(&json))),
        Err(Error { kind: ErrorKind::NotFound, .. }) => Ok(None),
        Err(e) => Err(format!("Lyrics unavailable: {e}")),
    }
}

fn parse(json: &serde_json::Value) -> Option<Lyrics> {
    let lyrics = &json["lyrics"];
    let synced = lyrics["syncType"].as_str() == Some("LINE_SYNCED");
    let mut lines = Vec::new();
    let mut times = Vec::new();
    for line in lyrics["lines"].as_array()? {
        let text = line["words"].as_str().unwrap_or_default().trim();
        // Spotify marks instrumental breaks with a lone note.
        if text.is_empty() || text == "\u{266a}" {
            continue;
        }
        let at = &line["startTimeMs"];
        times.push(at.as_str().and_then(|t| t.parse().ok()).or_else(|| at.as_u64().map(|n| n as u32)));
        lines.push(text.to_string());
    }
    if lines.is_empty() {
        return None;
    }
    let times_ms = synced.then(|| times.into_iter().collect::<Option<Vec<u32>>>()).flatten();
    Some(Lyrics { lines, times_ms })
}

/// Index of the line being sung at `position_ms` (the last one that has started), if any.
pub fn current_line(times_ms: &[u32], position_ms: u32) -> Option<usize> {
    times_ms.partition_point(|&t| t <= position_ms).checked_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_synced_and_skips_breaks() {
        let j = json!({"lyrics": {"syncType": "LINE_SYNCED", "lines": [
            {"startTimeMs": "1000", "words": "one"},
            {"startTimeMs": "2000", "words": "\u{266a}"},
            {"startTimeMs": "3000", "words": " two "}
        ]}});
        let l = parse(&j).unwrap();
        assert_eq!(l.lines, ["one", "two"]);
        assert_eq!(l.times_ms, Some(vec![1000, 3000]));
    }

    #[test]
    fn unsynced_or_partial_times_have_no_timing() {
        let unsynced = json!({"lyrics": {"syncType": "UNSYNCED", "lines": [{"startTimeMs": "0", "words": "a"}]}});
        assert_eq!(parse(&unsynced).unwrap().times_ms, None);
        let partial = json!({"lyrics": {"syncType": "LINE_SYNCED", "lines": [{"words": "a"}, {"startTimeMs": "5", "words": "b"}]}});
        assert_eq!(parse(&partial).unwrap().times_ms, None);
        assert!(parse(&json!({"lyrics": {"lines": []}})).is_none());
        assert!(parse(&json!({})).is_none());
    }

    #[test]
    fn finds_current_line() {
        let t = [1000, 3000, 5000];
        assert_eq!(current_line(&t, 0), None);
        assert_eq!(current_line(&t, 1000), Some(0));
        assert_eq!(current_line(&t, 4999), Some(1));
        assert_eq!(current_line(&t, 99_000), Some(2));
    }
}
