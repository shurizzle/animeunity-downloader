use anyhow::{anyhow, bail, Result};
use boa_engine::{js_str, value::JsValue, Context, Source};

use crate::RawVideo;

pub fn extract_video_infos(code: &str) -> Result<RawVideo> {
    let mut ctx = Context::default();
    match ctx
        .eval(Source::from_bytes(&code))
        .map_err(|e| anyhow!("{e}"))?
    {
        JsValue::Object(o) => {
            let url = match o
                .get(js_str!("url"), &mut ctx)
                .map_err(|e| anyhow!("{e}"))?
            {
                JsValue::String(s) => s.to_std_string()?.into_boxed_str(),
                _ => bail!("url not found"),
            };
            if url.is_empty() {
                bail!("url not found");
            }

            let file = match o
                .get(js_str!("file"), &mut ctx)
                .map_err(|e| anyhow!("{e}"))?
            {
                JsValue::String(s) => {
                    let s = s.to_std_string()?.into_boxed_str();
                    if s.is_empty() {
                        None
                    } else {
                        Some(s)
                    }
                }
                _ => None,
            };

            Ok(RawVideo { file, url })
        }
        _ => unreachable!(),
    }
}
