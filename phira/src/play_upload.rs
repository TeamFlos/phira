//! Score upload: the [`Uploader`] that talks to the server, and the binary
//! body of `POST /play/upload` it sends as `application/octet-stream`.
//!
//! # Sessions
//!
//! Every attempt first gets a nonce from `POST /play/session`, requested as
//! the attempt starts and carried in the upload. The server issues it bound to
//! the player and chart and accepts it once, so an upload proves an attempt
//! was actually started after the nonce was issued. It is also the challenge a
//! future device signature will cover: such a signature would be appended as
//! the last field and sign every byte before it, nonce included.
//!
//! # Body
//!
//! All integers are little-endian. A string is a `u16` byte length followed by
//! UTF-8, a blob a `u32` byte length followed by the bytes.
//!
//! | Field            | Encoding                                                 |
//! |------------------|----------------------------------------------------------|
//! | magic            | `b"PHRU"`                                                |
//! | format           | `u8`, currently [`FORMAT`]                               |
//! | chart            | `i32`                                                    |
//! | chart_updated    | `i64` microseconds since the epoch, `i64::MIN` if absent |
//! | session          | blob, the session nonce, empty if none was obtained      |
//! | client_version   | string                                                   |
//! | client_build     | string, the short git hash                               |
//! | platform         | string, `{os}-{arch}`                                    |
//! | flavor           | string, the distribution channel, see [`set_flavor`]    |
//! | token            | blob, the score token as is                              |
//! | play_record_codec| `u8`, see [`Codec`]                                      |
//! | play_record      | blob, empty when the codec is [`Codec::None`]            |
//!
//! New fields are only ever appended, and readers ignore trailing bytes, so
//! adding one does not break a server that predates it. [`FORMAT`] is bumped
//! only for changes an old reader would misread.
//!
//! `chart_updated` is in microseconds because the server compares it for
//! equality with a Postgres `timestamptz`, which has exactly that precision.
//!
//! The server-side reader lives in `phira-server/src/route/play.rs`; the test
//! vector below is shared with it.

use crate::{
    client::{recv_raw, Client},
    scene::RECORD_ID,
};
use anyhow::{ensure, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{DateTime, Utc};
use prpr::{
    play_record::PlayRecord,
    scene::{RecordUpdateState, Uploader},
    task::Task,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::{atomic::Ordering, Arc, LazyLock, OnceLock};

const MAGIC: &[u8; 4] = b"PHRU";
pub const FORMAT: u8 = 1;

const ZSTD_LEVEL: i32 = 3;

#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Codec {
    None = 0,
    /// The [`PlayRecord`] as JSON, compressed with zstd.
    ZstdJson = 1,
}

/// Identifies the build that produced an upload.
pub struct ClientInfo<'a> {
    pub version: &'a str,
    pub build: &'a str,
    pub platform: &'a str,
    pub flavor: &'a str,
}

impl ClientInfo<'static> {
    pub fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            build: env!("GIT_HASH"),
            platform: &PLATFORM,
            flavor: FLAVOR.get().map_or(FLAVOR_UNKNOWN, String::as_str),
        }
    }
}

/// The distribution channel, read from the `flavor` asset at startup.
static FLAVOR: OnceLock<String> = OnceLock::new();

/// Reported when an upload happens before [`set_flavor`] ran. Distinct from
/// the `none` used when the build ships no `flavor` asset at all.
const FLAVOR_UNKNOWN: &str = "unknown";

/// Records the distribution channel for every later upload. Loading assets
/// goes through macroquad, which belongs to the main thread, so it is read
/// once there rather than on the upload task. Only the first call counts.
pub fn set_flavor(flavor: String) {
    let _ = FLAVOR.set(flavor);
}

// HarmonyOS reports itself as `linux`; only the target env tells them apart.
#[cfg(target_env = "ohos")]
const OS: &str = "ohos";
#[cfg(not(target_env = "ohos"))]
const OS: &str = std::env::consts::OS;

static PLATFORM: LazyLock<String> = LazyLock::new(|| format!("{OS}-{}", std::env::consts::ARCH));

/// Uploads the plays of one chart to the Phira server.
pub struct ApiUploader {
    pub chart: i32,
    pub chart_updated: Option<DateTime<Utc>>,
}

impl Uploader for ApiUploader {
    fn open_session(&self) -> Task<Result<Vec<u8>>> {
        let chart = self.chart;
        Task::new(async move {
            #[derive(Deserialize)]
            struct Resp {
                nonce: String,
            }
            let resp: Resp = recv_raw(Client::post("/play/session", &json!({ "chart": chart }))).await?.json().await?;
            Ok(STANDARD.decode(resp.nonce)?)
        })
    }

    fn upload(&self, token: Vec<u8>, play_record: Option<Arc<PlayRecord>>, session: Option<Vec<u8>>) -> Task<Result<RecordUpdateState>> {
        let (chart, chart_updated) = (self.chart, self.chart_updated);
        Task::new(async move {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Resp {
                id: i32,
                exp_delta: f64,
                new_best: bool,
                improvement: u32,
                new_rks: f32,
            }
            let body = PlayUpload {
                chart,
                chart_updated,
                session: session.as_deref(),
                token: &token,
                play_record: play_record.as_deref(),
            }
            .encode(&ClientInfo::current())?;
            let resp: Resp = recv_raw(Client::post_binary("/play/upload", body)).await?.json().await?;
            RECORD_ID.store(resp.id, Ordering::Relaxed);
            Ok(RecordUpdateState {
                best: resp.new_best,
                improvement: resp.improvement,
                gain_exp: resp.exp_delta as f32,
                new_rks: Some(resp.new_rks),
            })
        })
    }
}

pub struct PlayUpload<'a> {
    pub chart: i32,
    pub chart_updated: Option<DateTime<Utc>>,
    pub session: Option<&'a [u8]>,
    pub token: &'a [u8],
    pub play_record: Option<&'a PlayRecord>,
}

impl PlayUpload<'_> {
    pub fn encode(&self, client: &ClientInfo) -> Result<Vec<u8>> {
        let (codec, record) = match self.play_record {
            Some(record) => (Codec::ZstdJson, zstd::encode_all(serde_json::to_vec(record)?.as_slice(), ZSTD_LEVEL)?),
            None => (Codec::None, Vec::new()),
        };

        let mut out = Vec::with_capacity(64 + self.token.len() + record.len());
        out.extend_from_slice(MAGIC);
        out.push(FORMAT);
        out.extend_from_slice(&self.chart.to_le_bytes());
        out.extend_from_slice(&self.chart_updated.map_or(i64::MIN, |it| it.timestamp_micros()).to_le_bytes());
        put_blob(&mut out, self.session.unwrap_or_default())?;
        put_str(&mut out, client.version)?;
        put_str(&mut out, client.build)?;
        put_str(&mut out, client.platform)?;
        put_str(&mut out, client.flavor)?;
        put_blob(&mut out, self.token)?;
        out.push(codec as u8);
        put_blob(&mut out, &record)?;
        Ok(out)
    }
}

fn put_str(out: &mut Vec<u8>, s: &str) -> Result<()> {
    ensure!(s.len() <= u16::MAX as usize, "string too long");
    out.extend_from_slice(&(s.len() as u16).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
    Ok(())
}

fn put_blob(out: &mut Vec<u8>, data: &[u8]) -> Result<()> {
    ensure!(data.len() <= u32::MAX as usize, "blob too long");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use prpr::play_record::{PlayMeta, PLAY_RECORD_VERSION};

    const CLIENT: ClientInfo<'static> = ClientInfo {
        version: "0.7.0",
        build: "abc1234",
        platform: "android-aarch64",
        flavor: "taptap",
    };

    /// Shared with the server's reader; change both or neither.
    const VECTOR: &str =
        "50485255012a00000040222018240a060004000000aabbccdd0500302e372e300700616263313233340f00616e64726f69642d616172636836340600746170746170030000000102030000000000";

    #[test]
    fn matches_the_shared_test_vector() {
        let upload = PlayUpload {
            chart: 42,
            chart_updated: DateTime::from_timestamp_micros(1_700_000_000_123_456),
            session: Some(&[0xaa, 0xbb, 0xcc, 0xdd]),
            token: &[1, 2, 3],
            play_record: None,
        };
        assert_eq!(hex::encode(upload.encode(&CLIENT).unwrap()), VECTOR);
    }

    #[test]
    fn the_play_record_round_trips_through_zstd() {
        let record = PlayRecord {
            version: PLAY_RECORD_VERSION,
            meta: PlayMeta::default(),
            notes: Vec::new(),
            inputs: Vec::new(),
        };
        let upload = PlayUpload {
            chart: 1,
            chart_updated: None,
            session: None,
            token: &[],
            play_record: Some(&record),
        };
        let data = upload.encode(&CLIENT).unwrap();

        // Header up to the codec: magic, format, chart, chart_updated, the
        // empty session, the four strings and the empty token.
        let codec_at = 4 + 1 + 4 + 8 + 4 + (2 + 5) + (2 + 7) + (2 + 15) + (2 + 6) + 4;
        assert_eq!(&data[9..17], &i64::MIN.to_le_bytes());
        assert_eq!(data[codec_at], Codec::ZstdJson as u8);
        let len = u32::from_le_bytes(data[codec_at + 1..codec_at + 5].try_into().unwrap()) as usize;
        let blob = &data[codec_at + 5..];
        assert_eq!(blob.len(), len);
        let json = zstd::decode_all(blob).unwrap();
        assert_eq!(json, serde_json::to_vec(&record).unwrap());
    }
}
