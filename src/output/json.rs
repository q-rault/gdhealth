use crate::error::Result;
use serde::Serialize;

pub fn render<T: Serialize>(value: &T) -> Result<String> {
    Ok(serde_json::to_string_pretty(value)?)
}
