use anyhow::{bail, Result};
use mini_v8::{FromValue, MiniV8};

use crate::Video;

pub fn type_name(value: &mini_v8::Value) -> &'static str {
    use mini_v8::Value;
    match value {
        Value::Undefined => "undefined",
        Value::Null => "null",
        Value::Boolean(_) => "boolean",
        Value::Number(_) => "number",
        Value::Date(_) => "date",
        Value::Function(_) => "function",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        Value::String(_) => "string",
    }
}

impl FromValue for Video {
    fn from_value(value: mini_v8::Value, mv8: &MiniV8) -> mini_v8::Result<Self> {
        let value = match value {
            mini_v8::Value::Object(v) => v,
            _ => {
                return Err(mini_v8::Error::FromJsConversionError {
                    from: type_name(&value),
                    to: "Video",
                })
            }
        };

        let file = value
            .get::<_, mini_v8::Value>("file")?
            .coerce_string(mv8)?
            .to_string()
            .into_boxed_str();
        let url = value
            .get::<_, mini_v8::Value>("url")?
            .coerce_string(mv8)?
            .to_string()
            .into_boxed_str();

        Ok(Self { file, url })
    }
}

pub fn extract_video_infos(code: &str) -> Result<Video> {
    let mv8 = MiniV8::new();
    match mv8.eval::<_, Video>(code) {
        Ok(x) => {
            if x.url.is_empty() {
                bail!("url not found");
            }
            if x.file.is_empty() {
                bail!("file not found");
            }
            Ok(x)
        }
        Err(err) => {
            bail!("{}", err)
        }
    }
}
