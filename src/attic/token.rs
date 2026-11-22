use std::collections::BTreeMap;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Datelike, Utc};
use serde::Deserialize;
use serde_json::Value;

use super::valid_name;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Malformed,
    Expiration,
    Expired,
    NotYetValid,
    Claims,
}

pub struct Token {
    pub expiration: DateTime<Utc>,
    pub cache_names: Vec<String>,
    pub has_patterns: bool,
}

impl Token {
    pub fn parse(input: &str, now: DateTime<Utc>) -> Result<Self, Error> {
        let mut segments = input.split('.');
        let header = segments.next().ok_or(Error::Malformed)?;
        let payload = segments.next().ok_or(Error::Malformed)?;
        let signature = segments.next().ok_or(Error::Malformed)?;
        if segments.next().is_some() || signature.is_empty() {
            return Err(Error::Malformed);
        }
        let header = decode(header)?;
        if !serde_json::from_slice::<Value>(&header).is_ok_and(|value| value.is_object()) {
            return Err(Error::Malformed);
        }
        // The server verifies the signature. Here only check its JWT encoding.
        decode(signature)?;
        let payload: Value =
            serde_json::from_slice(&decode(payload)?).map_err(|_| Error::Malformed)?;
        let payload = payload.as_object().ok_or(Error::Malformed)?;
        let expiration = payload
            .get("exp")
            .and_then(timestamp)
            .ok_or(Error::Expiration)?;
        if expiration <= now {
            return Err(Error::Expired);
        }
        if let Some(not_before) = payload.get("nbf")
            && timestamp(not_before).ok_or(Error::Malformed)? > now
        {
            return Err(Error::NotYetValid);
        }

        let claim = payload
            .get("https://jwt.attic.rs/v1")
            .ok_or(Error::Claims)?;
        let Claim { caches } = serde_json::from_value(claim.clone()).map_err(|_| Error::Claims)?;
        let has_patterns = caches.keys().any(|name| name.contains('*'));
        let cache_names = caches
            .into_iter()
            .filter_map(|(name, permission)| {
                (permission.read == 1 && valid_name(&name)).then_some(name)
            })
            .collect();
        Ok(Self {
            expiration,
            cache_names,
            has_patterns,
        })
    }
}

#[derive(Deserialize)]
struct Claim {
    caches: BTreeMap<String, Permission>,
}

#[derive(Deserialize)]
struct Permission {
    #[serde(rename = "r", default)]
    read: u8,
}

fn decode(segment: &str) -> Result<Vec<u8>, Error> {
    URL_SAFE_NO_PAD
        .decode(segment)
        .map_err(|_| Error::Malformed)
}

fn timestamp(value: &Value) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp(value.as_i64()?, 0).filter(|date| (0..=9999).contains(&date.year()))
}
