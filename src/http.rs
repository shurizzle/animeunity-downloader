use anyhow::Result;
use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(feature = "ureq")] {
        pub fn get(url: &str) -> Result<String> {
            Ok(ureq::get(url)
                .call()?
                .into_string()?)
        }
    } else if #[cfg(feature = "curl")] {
        use curl::easy::{Easy2, Handler};

        struct Collector(Vec<u8>);

        impl Handler for Collector {
            fn write(&mut self, data: &[u8]) -> std::result::Result<usize, curl::easy::WriteError> {
                self.0.extend_from_slice(data);
                Ok(data.len())
            }
        }

        pub fn get(url: &str) -> Result<String> {
            let mut curl = Easy2::new(Collector(Vec::new()));
            curl.get(true)?;
            curl.url(url)?;
            curl.perform()?;
            let content = core::mem::take(&mut curl.get_mut().0);
            Ok(String::from_utf8(content)?)
        }
    } else {
        compile_error!("No http client selected.");
    }
}
