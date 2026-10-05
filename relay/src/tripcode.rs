use base64::Engine;
use sha1::{Digest, Sha1};

use crate::error::RelayError;

fn vichan_salt(input: &str) -> String {
    let mut padded = input.as_bytes().to_vec();
    padded.extend_from_slice(b"H..");
    let mut salt = [padded[1], padded[2]];
    for byte in &mut salt {
        if !matches!(*byte, b'.'..=b'z') {
            *byte = b'.';
        }
        *byte = match *byte {
            b':' => b'A',
            b';' => b'B',
            b'<' => b'C',
            b'=' => b'D',
            b'>' => b'E',
            b'?' => b'F',
            b'@' => b'G',
            b'[' => b'a',
            b'\\' => b'b',
            b']' => b'c',
            b'^' => b'd',
            b'_' => b'e',
            b'`' => b'f',
            other => other,
        };
    }
    String::from_utf8(salt.to_vec()).expect("salt bytes are ascii")
}

pub fn tripcode(input: &str) -> Result<String, RelayError> {
    let salt = vichan_salt(input);
    let hash = pwhash::unix::crypt(input, &salt)
        .map_err(|error| RelayError::BadRequest(format!("tripcode crypt: {error:?}")))?;
    let text = hash.as_str();
    Ok(text[text.len().saturating_sub(10)..].to_string())
}

pub fn secure_tripcode(input: &str, key: &str) -> Result<String, RelayError> {
    let (encoded, _, had_errors) = encoding_rs::SHIFT_JIS.encode(input);
    if had_errors {
        return Err(RelayError::BadRequest("secure tripcode: shift_jis encoding failed".into()));
    }
    let key_bytes = encoded.as_ref();
    let mut hasher = Sha1::new();
    hasher.update(key_bytes);
    hasher.update(key.as_bytes());
    let digest = hasher.finalize();
    let encoded_digest = base64::engine::general_purpose::STANDARD.encode(digest);
    let salt_part = encoded_digest
        .get(..4)
        .ok_or_else(|| RelayError::Internal("secure tripcode: bad digest".into()))?
        .replace('+', ".");
    let setting = format!("_..A.{salt_part}");
    let hash = pwhash::unix::crypt(key_bytes, &setting)
        .map_err(|error| RelayError::BadRequest(format!("secure tripcode: {error:?}")))?;
    let text = hash.as_str();
    Ok(text[text.len().saturating_sub(10)..].to_string())
}

pub fn resolve(raw: &str, key: &str) -> Result<forum_model::Tripcode, RelayError> {
    if let Some(seed) = raw.strip_prefix("##") {
        let trip = secure_tripcode(seed, key)?;
        Ok(forum_model::Tripcode {
            secured: true,
            trip: trip.as_bytes().to_vec(),
        })
    } else if let Some(seed) = raw.strip_prefix('#') {
        let trip = tripcode(seed)?;
        Ok(forum_model::Tripcode {
            secured: false,
            trip: trip.as_bytes().to_vec(),
        })
    } else {
        Err(RelayError::BadRequest("tripcode must start with # or ##".into()))
    }
}
