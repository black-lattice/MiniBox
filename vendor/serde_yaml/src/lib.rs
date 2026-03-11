use serde::Serialize;
use serde::de::DeserializeOwned;

pub use serde_json::Error;

pub fn from_str<T>(value: &str) -> Result<T, Error>
where
    T: DeserializeOwned,
{
    serde_json::from_str(value)
}

pub fn to_string<T>(value: &T) -> Result<String, Error>
where
    T: Serialize,
{
    serde_json::to_string_pretty(value)
}
