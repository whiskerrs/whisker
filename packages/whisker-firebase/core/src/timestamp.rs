use crate::{FirebaseError, Result};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::SerializeTupleStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Serde newtype name recognised by the Firestore serializer.
#[doc(hidden)]
pub const TIMESTAMP_NAME: &str = "$__whisker_firebase_timestamp";

const MIN_SECONDS: i64 = -62_135_596_800; // 0001-01-01T00:00:00Z
const MAX_SECONDS: i64 = 253_402_300_799; // 9999-12-31T23:59:59Z

/// A point in time with nanosecond resolution, matching Firebase's `Timestamp`.
///
/// Valid values lie between 0001-01-01 and 9999-12-31 UTC. Firestore stores
/// microseconds, so sub-microsecond digits are truncated on write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp {
    seconds: i64,
    nanoseconds: u32,
}

impl Timestamp {
    /// Seconds and nanoseconds since the Unix epoch.
    pub fn new(seconds: i64, nanoseconds: u32) -> Result<Self> {
        if !(MIN_SECONDS..=MAX_SECONDS).contains(&seconds) || nanoseconds >= 1_000_000_000 {
            return Err(FirebaseError::invalid_argument(
                "app",
                "timestamps must lie between 0001-01-01 and 9999-12-31 UTC",
            ));
        }
        Ok(Self {
            seconds,
            nanoseconds,
        })
    }

    pub fn now() -> Self {
        Self::from(SystemTime::now())
    }

    pub fn from_millis(millis: i64) -> Result<Self> {
        Self::new(
            millis.div_euclid(1000),
            (millis.rem_euclid(1000) * 1_000_000) as u32,
        )
    }

    pub fn seconds(&self) -> i64 {
        self.seconds
    }

    pub fn nanoseconds(&self) -> u32 {
        self.nanoseconds
    }

    pub fn to_millis(&self) -> i64 {
        self.seconds * 1000 + i64::from(self.nanoseconds / 1_000_000)
    }

    pub fn to_system_time(&self) -> SystemTime {
        if self.seconds >= 0 {
            UNIX_EPOCH + Duration::new(self.seconds as u64, self.nanoseconds)
        } else {
            UNIX_EPOCH - Duration::from_secs(self.seconds.unsigned_abs())
                + Duration::from_nanos(self.nanoseconds.into())
        }
    }
}

impl From<SystemTime> for Timestamp {
    /// Clamps to the representable range.
    fn from(time: SystemTime) -> Self {
        let (seconds, nanoseconds) = match time.duration_since(UNIX_EPOCH) {
            Ok(after) => (
                i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
                after.subsec_nanos(),
            ),
            Err(before) => {
                let before = before.duration();
                let mut seconds = -i64::try_from(before.as_secs()).unwrap_or(i64::MAX);
                let mut nanos = before.subsec_nanos();
                if nanos > 0 {
                    seconds -= 1;
                    nanos = 1_000_000_000 - nanos;
                }
                (seconds, nanos)
            }
        };
        if seconds < MIN_SECONDS {
            return Self {
                seconds: MIN_SECONDS,
                nanoseconds: 0,
            };
        }
        if seconds > MAX_SECONDS {
            return Self {
                seconds: MAX_SECONDS,
                nanoseconds: 999_999_999,
            };
        }
        Self {
            seconds,
            nanoseconds,
        }
    }
}

impl From<Timestamp> for SystemTime {
    fn from(timestamp: Timestamp) -> Self {
        timestamp.to_system_time()
    }
}

/// Serialized as a named newtype over `(seconds, nanoseconds)` so the Firestore
/// serializer stores a native timestamp. Other formats see a two-element tuple.
impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        struct Parts(i64, u32);
        impl Serialize for Parts {
            fn serialize<S: Serializer>(
                &self,
                serializer: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                let mut tuple = serializer.serialize_tuple_struct("Timestamp", 2)?;
                tuple.serialize_field(&self.0)?;
                tuple.serialize_field(&self.1)?;
                tuple.end()
            }
        }
        serializer.serialize_newtype_struct(TIMESTAMP_NAME, &Parts(self.seconds, self.nanoseconds))
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        deserializer.deserialize_newtype_struct(TIMESTAMP_NAME, TimestampVisitor)
    }
}

struct TimestampVisitor;
impl<'de> Visitor<'de> for TimestampVisitor {
    type Value = Timestamp;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a Firebase timestamp")
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Timestamp, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Timestamp, A::Error> {
        let seconds = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        let nanoseconds = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(1, &self))?;
        Timestamp::new(seconds, nanoseconds).map_err(de::Error::custom)
    }

    /// Accepts `{seconds, nanoseconds}` and the single-key form used when a
    /// timestamp passes through a self-describing buffer (e.g. untagged enums).
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<Timestamp, A::Error> {
        let mut seconds = None;
        let mut nanoseconds = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                TIMESTAMP_NAME => {
                    let (s, n): (i64, u32) = map.next_value()?;
                    return Timestamp::new(s, n).map_err(de::Error::custom);
                }
                "seconds" => seconds = Some(map.next_value()?),
                "nanoseconds" => nanoseconds = Some(map.next_value()?),
                _ => {
                    map.next_value::<de::IgnoredAny>()?;
                }
            }
        }
        Timestamp::new(
            seconds.ok_or_else(|| de::Error::missing_field("seconds"))?,
            nanoseconds.unwrap_or(0),
        )
        .map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_system_time_before_and_after_the_epoch() {
        for millis in [-1_500, -1, 0, 1, 1_700_000_000_123] {
            let timestamp = Timestamp::from_millis(millis).unwrap();
            assert_eq!(timestamp.to_millis(), millis);
            assert_eq!(Timestamp::from(timestamp.to_system_time()), timestamp);
        }
        assert!(Timestamp::new(0, 1_000_000_000).is_err());
        assert!(Timestamp::new(MAX_SECONDS + 1, 0).is_err());
    }
}
