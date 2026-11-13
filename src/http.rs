use anyhow::Result;
use cfg_if::cfg_if;

// The video embed host (vixcloud) answers 403 when the Referer header is missing.
const REFERER: &str = crate::domain!("https://" _ "/");

cfg_if! {
    if #[cfg(feature = "ureq")] {
        use std::sync::LazyLock;

        fn agent() -> &'static ureq::Agent {
            static AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
                let config = ureq::Agent::config_builder();
                #[cfg(feature = "ureq-native-tls")]
                let config = config.tls_config(
                    ureq::tls::TlsConfig::builder()
                        .provider(ureq::tls::TlsProvider::NativeTls)
                        .build(),
                );
                config.build().into()
            });
            &AGENT
        }

        pub fn get(url: &str) -> Result<String> {
            Ok(agent()
                .get(url)
                .header("Referer", REFERER)
                .call()?
                .body_mut()
                .read_to_string()?)
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
            curl.referer(REFERER)?;
            curl.fail_on_error(true)?;
            curl.perform()?;
            let content = core::mem::take(&mut curl.get_mut().0);
            Ok(String::from_utf8(content)?)
        }
    } else {
        compile_error!("No http client selected.");
    }
}
