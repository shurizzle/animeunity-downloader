use anyhow::{bail, Result};
use quickjs_runtime::{builder::QuickJsRuntimeBuilder, jsutils::Script, values::JsValueFacade};

use crate::Video;

pub fn extract_video_infos(code: &str) -> Result<Video> {
    let x: Video = {
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
    if x.file.is_empty() {
        bail!("file not found");
    }
    Ok(x)
}
