//! Henrri webhooks: the event catalogue, signature verification, and a small
//! inbox of the deliveries this server received.
//!
//! Henrri POSTs a JSON envelope to a callback URL configured in its interface,
//! signed with `HMAC-SHA256(secret, "{timestamp}.{rawBody}")`, hex lowercase,
//! in `X-Henrri-Webhook-Signature: sha256=<hex>`. There is no route to manage
//! subscriptions and no retry: a delivery is sent once. When this server runs
//! over HTTP it accepts deliveries at `/webhooks`, verifies them, and keeps the
//! last [`INBOX_CAPACITY`] for the `list_received_webhook_events` tool.

use std::{
    collections::VecDeque,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;

/// The header carrying `sha256=<hex>`.
pub const SIGNATURE_HEADER: &str = "x-henrri-webhook-signature";
/// The header carrying the delivery GUID.
pub const DELIVERY_HEADER: &str = "x-henrri-webhook-delivery";
/// The header carrying the unix timestamp the signature covers.
pub const TIMESTAMP_HEADER: &str = "x-henrri-webhook-timestamp";

/// How many deliveries the inbox keeps; older ones are dropped first.
pub const INBOX_CAPACITY: usize = 200;
/// How far a delivery's timestamp may be from now before it is refused as a replay.
pub const DEFAULT_TOLERANCE: Duration = Duration::from_secs(300);
/// The largest delivery body accepted. Henrri's envelope is a few hundred bytes.
pub const MAX_BODY_BYTES: usize = 64 * 1024;

/// One event Henrri can send.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct EventType {
    /// The event name, as `event` carries it.
    pub event: &'static str,
    /// The kind of resource `id` refers to.
    pub resource: &'static str,
    /// When it is sent.
    pub description: &'static str,
    /// The tool that reads the resource, from the envelope's `id`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_with: Option<&'static str>,
}

/// Every event of the v1 specification, plus the test ping.
pub const EVENTS: [EventType; 13] = [
    EventType {
        event: "customer.created",
        resource: "customer",
        description: "A customer was created, in Henrri or through the API.",
        read_with: Some("get_customer"),
    },
    EventType {
        event: "customer.updated",
        resource: "customer",
        description: "A customer was modified.",
        read_with: Some("get_customer"),
    },
    EventType {
        event: "customer.deleted",
        resource: "customer",
        description: "A customer was deleted; it can no longer be read.",
        read_with: None,
    },
    EventType {
        event: "document.created",
        resource: "document",
        description: "A document (quote, invoice…) was created as a draft.",
        read_with: Some("get_document"),
    },
    EventType {
        event: "document.updated",
        resource: "document",
        description: "A document or its lines were modified.",
        read_with: Some("get_document"),
    },
    EventType {
        event: "document.finalized",
        resource: "document",
        description: "A document was finalized: it has its definitive number.",
        read_with: Some("get_document"),
    },
    EventType {
        event: "document.validated",
        resource: "document",
        description: "A quotation was validated (accepted by the customer).",
        read_with: Some("get_document"),
    },
    EventType {
        event: "document.deleted",
        resource: "document",
        description: "A document was deleted.",
        read_with: None,
    },
    EventType {
        event: "item.created",
        resource: "item",
        description: "An item was added to the catalogue.",
        read_with: Some("get_item"),
    },
    EventType {
        event: "item.updated",
        resource: "item",
        description: "An item was modified.",
        read_with: Some("get_item"),
    },
    EventType {
        event: "item.deleted",
        resource: "item",
        description: "An item was deleted.",
        read_with: None,
    },
    EventType {
        event: "payment.created",
        resource: "payment",
        description: "A payment was recorded.",
        read_with: Some("get_payment"),
    },
    EventType {
        event: "webhook.test",
        resource: "none",
        description: "The test button in Henrri's webhook settings: same envelope and signature, id 0.",
        read_with: None,
    },
];

/// The envelope Henrri POSTs.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    /// The event name, e.g. `document.finalized`.
    pub event: Option<String>,
    /// When the event occurred.
    pub date: Option<String>,
    /// The affected resource's id (0 for a test ping).
    pub id: Option<i64>,
    /// The company the event comes from.
    pub company_id: Option<i64>,
    /// The delivery GUID, also in the `X-Henrri-Webhook-Delivery` header.
    pub delivery: Option<String>,
}

/// A delivery that passed verification.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ReceivedEvent {
    /// The envelope.
    #[serde(flatten)]
    pub envelope: Envelope,
    /// When this server received it, RFC 3339.
    pub received_at: String,
}

/// Why a signature did not verify, or that it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// The signature matches and the timestamp is recent.
    Valid,
    /// The header is not `sha256=<64 hex digits>`.
    Malformed,
    /// The signature does not match: wrong secret, or a modified body.
    Mismatch,
    /// The timestamp is not a number of seconds.
    BadTimestamp,
    /// The timestamp is too far from now: a replay, or a wrong clock.
    Stale,
}

impl Verdict {
    /// One sentence for a person.
    pub fn explain(self) -> &'static str {
        match self {
            Self::Valid => "the signature is valid and the delivery is recent",
            Self::Malformed => "the signature header is not sha256=<64 hex digits>",
            Self::Mismatch => {
                "the signature does not match: another secret signed it, or the body was modified (verify the raw bytes, not re-serialized JSON)"
            }
            Self::BadTimestamp => "the timestamp is not a unix time in seconds",
            Self::Stale => {
                "the timestamp is too far from now: a replayed delivery, or a clock out of sync"
            }
        }
    }
}

/// Check a delivery's signature, in constant time.
///
/// `body` must be the raw bytes received: a re-serialized JSON object does not
/// have the same signature.
pub fn verify(
    secret: &str,
    timestamp: &str,
    body: &[u8],
    signature: &str,
    tolerance: Duration,
    now: SystemTime,
) -> Verdict {
    let Some(expected) = signature
        .trim()
        .strip_prefix("sha256=")
        .and_then(decode_hex)
        .filter(|bytes| bytes.len() == 32)
    else {
        return Verdict::Malformed;
    };
    let Ok(seconds) = timestamp.trim().parse::<u64>() else {
        return Verdict::BadTimestamp;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return Verdict::Mismatch;
    };
    mac.update(timestamp.trim().as_bytes());
    mac.update(b".");
    mac.update(body);
    if mac.verify_slice(&expected).is_err() {
        return Verdict::Mismatch;
    }
    let sent = UNIX_EPOCH + Duration::from_secs(seconds);
    let skew = now
        .duration_since(sent)
        .or_else(|e| Ok::<_, ()>(e.duration()))
        .unwrap_or(Duration::MAX);
    if skew > tolerance {
        return Verdict::Stale;
    }
    Verdict::Valid
}

/// Sign a body the way Henrri does, which the tests and `--check` use.
pub fn sign(secret: &str, timestamp: &str, body: &[u8]) -> String {
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return String::new();
    };
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(body);
    let digest = mac.finalize().into_bytes();
    let mut hex = String::with_capacity(7 + digest.len() * 2);
    hex.push_str("sha256=");
    for byte in digest {
        hex.push(char::from(HEX[usize::from(byte >> 4)]));
        hex.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    hex
}

const HEX: &[u8; 16] = b"0123456789abcdef";

fn decode_hex(raw: &str) -> Option<Vec<u8>> {
    if !raw.len().is_multiple_of(2) {
        return None;
    }
    let nibble = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    raw.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[high, low]| Some((nibble(*high)? << 4) | nibble(*low)?))
        .collect()
}

/// The deliveries this process received, newest last, bounded.
///
/// A plain mutex: every critical section is a push or a copy, never an await.
#[derive(Debug, Default)]
pub struct Inbox {
    events: Mutex<VecDeque<ReceivedEvent>>,
}

/// What [`Inbox::accept`] did with a delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accepted {
    /// Stored.
    New,
    /// Already stored under the same delivery GUID: ignored.
    Duplicate,
}

impl Inbox {
    /// Store a verified delivery, dropping the oldest past capacity.
    pub fn accept(&self, envelope: Envelope) -> Accepted {
        let Ok(mut events) = self.events.lock() else {
            return Accepted::Duplicate;
        };
        if let Some(delivery) = envelope.delivery.as_deref()
            && events
                .iter()
                .any(|e| e.envelope.delivery.as_deref() == Some(delivery))
        {
            return Accepted::Duplicate;
        }
        if events.len() >= INBOX_CAPACITY {
            events.pop_front();
        }
        events.push_back(ReceivedEvent {
            envelope,
            received_at: chrono::Utc::now().to_rfc3339(),
        });
        Accepted::New
    }

    /// The stored deliveries, newest first, optionally of one event only.
    pub fn recent(&self, event: Option<&str>, limit: usize) -> Vec<ReceivedEvent> {
        let Ok(events) = self.events.lock() else {
            return Vec::new();
        };
        events
            .iter()
            .rev()
            .filter(|e| event.is_none_or(|wanted| e.envelope.event.as_deref() == Some(wanted)))
            .take(limit)
            .cloned()
            .collect()
    }

    /// How many deliveries are stored.
    pub fn len(&self) -> usize {
        self.events.lock().map(|e| e.len()).unwrap_or_default()
    }

    /// True when nothing was received.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Parse a verified body; a body that is not the envelope is kept as raw JSON
/// under nothing, which the receiver refuses.
pub fn parse_envelope(body: &[u8]) -> Option<Envelope> {
    let value: Value = serde_json::from_slice(body).ok()?;
    serde_json::from_value(value).ok()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot fail loudly is not a test"
    )]
    use super::*;

    fn to_hex(bytes: &[u8]) -> String {
        bytes.iter().fold(String::new(), |mut out, b| {
            out.push(char::from(HEX[usize::from(b >> 4)]));
            out.push(char::from(HEX[usize::from(b & 0x0f)]));
            out
        })
    }

    const BODY: &[u8] = br#"{"event":"document.finalized","date":"2026-01-31T15:20:00+01:00","id":1245,"companyId":12,"delivery":"4734794d-065d-4879-83f4-6760416e84d7"}"#;

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    #[test]
    fn a_signature_made_like_henrri_verifies() {
        let signature = sign("s3cret", "1794242300", BODY);
        assert!(signature.starts_with("sha256="));
        assert_eq!(signature.len(), 7 + 64);
        let verdict = verify(
            "s3cret",
            "1794242300",
            BODY,
            &signature,
            DEFAULT_TOLERANCE,
            at(1_794_242_310),
        );
        assert_eq!(verdict, Verdict::Valid);
    }

    #[test]
    fn the_signature_is_hmac_sha256_of_timestamp_dot_body() {
        // RFC-style known vector, to pin the primitive itself.
        let mut mac = Hmac::<Sha256>::new_from_slice(b"key").unwrap();
        mac.update(b"The quick brown fox jumps over the lazy dog");
        let hex = to_hex(&mac.finalize().into_bytes());
        assert_eq!(
            hex,
            "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
        // And the message is "{timestamp}.{body}".
        let mut mac = Hmac::<Sha256>::new_from_slice(b"key").unwrap();
        mac.update(b"1794242300.{}");
        let expected = to_hex(&mac.finalize().into_bytes());
        assert_eq!(
            sign("key", "1794242300", b"{}"),
            format!("sha256={expected}")
        );
    }

    #[test]
    fn tampering_a_wrong_secret_or_an_old_delivery_is_refused() {
        let signature = sign("s3cret", "1794242300", BODY);
        let now = at(1_794_242_310);
        let mut tampered = BODY.to_vec();
        tampered[10] ^= 1;
        assert_eq!(
            verify(
                "s3cret",
                "1794242300",
                &tampered,
                &signature,
                DEFAULT_TOLERANCE,
                now
            ),
            Verdict::Mismatch
        );
        assert_eq!(
            verify(
                "other",
                "1794242300",
                BODY,
                &signature,
                DEFAULT_TOLERANCE,
                now
            ),
            Verdict::Mismatch
        );
        assert_eq!(
            verify(
                "s3cret",
                "1794242301",
                BODY,
                &signature,
                DEFAULT_TOLERANCE,
                now
            ),
            Verdict::Mismatch
        );
        assert_eq!(
            verify(
                "s3cret",
                "1794242300",
                BODY,
                &signature,
                DEFAULT_TOLERANCE,
                at(1_794_250_000)
            ),
            Verdict::Stale
        );
        assert_eq!(
            verify(
                "s3cret",
                "1794242300",
                BODY,
                "sha256=zz",
                DEFAULT_TOLERANCE,
                now
            ),
            Verdict::Malformed
        );
        assert_eq!(
            verify("s3cret", "noon", BODY, &signature, DEFAULT_TOLERANCE, now),
            Verdict::BadTimestamp
        );
    }

    #[test]
    fn uppercase_hex_is_accepted() {
        let signature = sign("s3cret", "1794242300", BODY)
            .to_uppercase()
            .replace("SHA256=", "sha256=");
        assert_eq!(
            verify(
                "s3cret",
                "1794242300",
                BODY,
                &signature,
                DEFAULT_TOLERANCE,
                at(1_794_242_300)
            ),
            Verdict::Valid
        );
    }

    #[test]
    fn the_inbox_deduplicates_and_stays_bounded() {
        let inbox = Inbox::default();
        let envelope = parse_envelope(BODY).unwrap();
        assert_eq!(inbox.accept(envelope.clone()), Accepted::New);
        assert_eq!(inbox.accept(envelope), Accepted::Duplicate);
        for i in 0..(INBOX_CAPACITY + 10) {
            inbox.accept(Envelope {
                event: Some("item.created".into()),
                date: None,
                id: Some(i64::try_from(i).unwrap()),
                company_id: Some(1),
                delivery: Some(format!("d-{i}")),
            });
        }
        assert_eq!(inbox.len(), INBOX_CAPACITY);
        let recent = inbox.recent(Some("item.created"), 3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].envelope.delivery.as_deref(), Some("d-209"));
        assert!(
            inbox.recent(Some("document.finalized"), 10).is_empty(),
            "evicted"
        );
    }

    #[test]
    fn the_catalogue_names_every_event_of_the_spec() {
        let names: Vec<&str> = EVENTS.iter().map(|e| e.event).collect();
        for event in [
            "customer.created",
            "customer.deleted",
            "customer.updated",
            "document.created",
            "document.deleted",
            "document.finalized",
            "document.updated",
            "document.validated",
            "item.created",
            "item.deleted",
            "item.updated",
            "payment.created",
        ] {
            assert!(names.contains(&event), "{event} is missing");
        }
    }
}
