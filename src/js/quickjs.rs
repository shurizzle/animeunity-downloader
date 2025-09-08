use anyhow::{bail, Result};
use quickjs_runtime::{builder::QuickJsRuntimeBuilder, jsutils::Script, values::JsValueFacade};

use crate::RawVideo;

pub fn extract_video_infos(code: &str) -> Result<RawVideo> {
    let mut x: RawVideo = {
        let rt = QuickJsRuntimeBuilder::new().build();
        match rt.eval_sync(None, Script::new("<main>", code))? {
            JsValueFacade::JsObject { cached_object } => serde_json::from_value(
                cached_object
                    .with_obj_sync(|realm, obj| realm.value_adapter_to_serde_value(obj))??,
            )?,
            _ => unreachable!(),
        }
    };
    if x.url.is_empty() {
        bail!("url not found");
    }
    x.file = match x.file {
        Some(f) if f.is_empty() => None,
        Some(f) => Some(f),
        None => None,
    };
    Ok(x)
}
