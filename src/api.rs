use std::collections::HashMap;
use std::io::Read;
use std::time::Duration;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};

// Flexible number deserializers: the tracker sends ints or floats inconsistently
// depending on endpoint, so every numeric field is deserialized into f64 via
// deserialize_any.

fn de_opt_f64<'de, D>(d: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    struct NumVisitor;
    impl<'de> Visitor<'de> for NumVisitor {
        type Value = Option<f64>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a number or null")
        }
        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D>(self, d: D) -> Result<Self::Value, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            d.deserialize_any(Inner)
        }
    }
    struct Inner;
    impl<'de> Visitor<'de> for Inner {
        type Value = Option<f64>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a number")
        }
        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
            Ok(Some(v as f64))
        }
        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
            Ok(Some(v as f64))
        }
        fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E> {
            Ok(Some(v))
        }
    }
    d.deserialize_option(NumVisitor)
}

fn de_opt_map_f64<'de, D>(d: D) -> Result<Option<HashMap<String, f64>>, D::Error>
where
    D: Deserializer<'de>,
{
    struct MapVisitor;
    impl<'de> Visitor<'de> for MapVisitor {
        type Value = Option<HashMap<String, f64>>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a map or null")
        }
        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D>(self, d: D) -> Result<Self::Value, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            struct Inner;
            impl<'de> Visitor<'de> for Inner {
                type Value = HashMap<String, f64>;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("a map")
                }
                fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
                where
                    A: MapAccess<'de>,
                {
                    let mut out = HashMap::new();
                    while let Some((k, v)) = map.next_entry::<String, serde_json::Value>()? {
                        let n = match v {
                            serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0),
                            _ => 0.0,
                        };
                        out.insert(k, n);
                    }
                    Ok(out)
                }
            }
            d.deserialize_map(Inner).map(Some)
        }
    }
    d.deserialize_option(MapVisitor)
}

fn de_opt_map_u64<'de, D>(d: D) -> Result<Option<HashMap<String, u64>>, D::Error>
where
    D: Deserializer<'de>,
{
    struct MapVisitor;
    impl<'de> Visitor<'de> for MapVisitor {
        type Value = Option<HashMap<String, u64>>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a map or null")
        }
        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D>(self, d: D) -> Result<Self::Value, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            struct Inner;
            impl<'de> Visitor<'de> for Inner {
                type Value = HashMap<String, u64>;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("a map")
                }
                fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
                where
                    A: MapAccess<'de>,
                {
                    let mut out = HashMap::new();
                    while let Some((k, v)) = map.next_entry::<String, serde_json::Value>()? {
                        let n = match v {
                            serde_json::Value::Number(n) => n.as_u64().unwrap_or(0),
                            _ => 0,
                        };
                        out.insert(k, n);
                    }
                    Ok(out)
                }
            }
            d.deserialize_map(Inner).map(Some)
        }
    }
    d.deserialize_option(MapVisitor)
}

// --- wire types ---

#[derive(Deserialize)]
pub struct RawCounts {
    pub done: Option<u64>,
    pub out: Option<u64>,
    pub todo: Option<u64>,
    pub rcr: Option<f64>,
    pub rcsr: Option<f64>,
    pub irsr: Option<f64>,
}

#[derive(Deserialize)]
pub struct RawStats {
    #[serde(default, deserialize_with = "de_opt_map_u64")]
    pub queues: Option<HashMap<String, u64>>,
}

#[derive(Deserialize)]
pub struct StatsJson {
    pub downloaders: Vec<String>,
    #[serde(default, deserialize_with = "de_opt_map_f64")]
    pub downloader_bytes: Option<HashMap<String, f64>>,
    #[serde(default, deserialize_with = "de_opt_map_u64")]
    pub downloader_count: Option<HashMap<String, u64>>,
    #[serde(default, deserialize_with = "de_opt_map_f64")]
    pub domain_bytes: Option<HashMap<String, f64>>,
    pub counts: RawCounts,
}

/// One live/recent log_message. All fields optional — different trackers and
/// code paths emit different subsets.
#[derive(Deserialize, Default)]
pub struct RawMsg {
    pub downloader: Option<String>,
    pub item: Option<String>,
    pub items: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    pub bytes: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_map_f64")]
    pub domain_bytes: Option<HashMap<String, f64>>,
    pub counts: Option<RawCounts>,
    pub stats: Option<RawStats>,
    pub ts: Option<f64>,
    pub is_duplicate: Option<bool>,
    pub user_agent: Option<String>,
}

pub fn http_get(url: &str, timeout: Duration) -> Result<String, String> {
    let resp = ureq::get(url)
        .timeout(timeout)
        .set("User-Agent", "attui (+https://tracker.archiveteam.org)")
        .call()
        .map_err(|e| format!("{e}"))?;
    let mut body = String::new();
    resp.into_reader()
        .take(64 * 1024 * 1024)
        .read_to_string(&mut body)
        .map_err(|e| format!("{e}"))?;
    Ok(body)
}

pub fn fetch_stats(api_base: &str, project: &str) -> Result<StatsJson, String> {
    let url = format!("{}/{project}/stats.json", api_base.trim_end_matches('/'));
    let body = http_get(&url, Duration::from_secs(20))?;
    serde_json::from_str(&body).map_err(|e| format!("bad stats.json: {e}"))
}

pub fn fetch_recent(log_host: &str, project: &str) -> Result<Vec<RawMsg>, String> {
    let url = format!("https://{log_host}/recent/{project}-log");
    let body = http_get(&url, Duration::from_secs(20))?;
    let mut msgs: Vec<RawMsg> =
        serde_json::from_str(&body).map_err(|e| format!("bad recent: {e}"))?;
    msgs.retain(|m| m.downloader.is_some() && m.item.is_some());
    msgs.sort_by(|a, b| a.ts.unwrap_or(0.0).total_cmp(&b.ts.unwrap_or(0.0)));
    Ok(msgs)
}
