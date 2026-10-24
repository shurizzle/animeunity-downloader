use anyhow::{anyhow, bail, Result};
use boa_engine::{js_string, Context, Source};

use crate::RawVideo;

pub fn extract_video_infos(code: &str) -> Result<RawVideo> {
    let mut ctx = Context::default();
    let result = ctx
        .eval(Source::from_bytes(&code))
        .map_err(|e| anyhow!("{e}"))?;

    let Some(o) = result.as_object() else {
        bail!("expected object");
    };

    let url = match o
        .get(js_string!("url"), &mut ctx)
        .map_err(|e| anyhow!("{e}"))?
        .as_string()
    {
        Some(s) => s.to_std_string()?.into_boxed_str(),
        None => bail!("url not found"),
    };
    if url.is_empty() {
        bail!("url not found");
    }

    let file = o
        .get(js_string!("file"), &mut ctx)
        .map_err(|e| anyhow!("{e}"))?
        .as_string()
        .map(|s| s.to_std_string())
        .transpose()?
        .map(String::into_boxed_str)
        .filter(|s| !s.is_empty());

    Ok(RawVideo { file, url })
}
