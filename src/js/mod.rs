use cfg_if::cfg_if;

#[allow(unused_macros)]
macro_rules! imp {
    ($file:literal) => {
        #[path = $file]
        mod imp;

        use crate::RawVideo;
        use anyhow::Result;

        pub fn extract_video_infos(mut code: String) -> Result<RawVideo> {
            code.push_str(
                "({file:window.video.filename||window.video.name,url:window.downloadUrl})",
            );
            imp::extract_video_infos(&code)
        }
    };
}

cfg_if! {
    if #[cfg(feature = "v8")] {
        imp!("v8.rs");
    } else if #[cfg(feature = "boa")] {
        imp!("boa.rs");
    } else if #[cfg(any(feature = "quickjs", feature = "quickjs-ng"))] {
        imp!("quickjs.rs");
    } else {
        compile_error!("No js engine selected.");
    }
}
