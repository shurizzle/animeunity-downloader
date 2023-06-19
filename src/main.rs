mod config;

use std::{
    collections::HashMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};
use base64::Engine;
use dialoguer::{theme::ColorfulTheme, MultiSelect};
use directories::ProjectDirs;
use serde::{Deserialize, Deserializer};
use url::Url;

#[derive(Debug, Deserialize)]
pub struct Episode {
    pub id: u64,
    pub number: String,
    pub scws_id: u64,
    pub file_name: String,
}

#[derive(Debug, Deserialize)]
pub struct Info {
    pub episodes_count: u64,
    pub episodes: Vec<Episode>,
}

pub trait PadLeft {
    fn pad_left(&mut self, size: usize);
}

impl PadLeft for String {
    fn pad_left(&mut self, size: usize) {
        if size <= self.len() {
            return;
        }

        let pad_len = size - self.len();
        unsafe {
            let buf = self.as_mut_vec();
            buf.reserve_exact(pad_len);
            std::ptr::copy(buf.as_ptr(), buf.as_mut_ptr().add(pad_len), buf.len());
            std::ptr::write_bytes(buf.as_mut_ptr(), b'0', pad_len);
            buf.set_len(size);
            buf.shrink_to_fit();
        }
    }
}

fn fetch_info(id: u64) -> impl Iterator<Item = Result<Episode>> {
    fn fetch_info_page(id: u64, start: u64, stop: u64) -> Result<Info> {
        let url = format!(
            "https://www.animeunity.it/info_api/{}/1?start_range={}&end_range={}",
            id, start, stop
        );

        let body = ureq::get(&url)
            .call()
            .context("Invalid informations")?
            .into_string()
            .context("Invalid informations")?;

        serde_json::from_slice(body.as_bytes()).context("Invalid informations")
    }

    fn num_len(mut n: u64) -> usize {
        if n == 0 {
            return 1;
        }

        let mut len = 0;
        while n > 0 {
            n /= 10;
            len += 1;
        }
        len
    }

    struct Pages {
        current: u64,
        max: u64,
    }

    impl Pages {
        #[inline(always)]
        pub fn new(max: u64) -> Self {
            Self { current: 1, max }
        }
    }

    impl Iterator for Pages {
        type Item = (u64, u64);

        fn next(&mut self) -> Option<Self::Item> {
            if self.current < self.max {
                let start = self.current;
                self.current += 120;
                let stop = (self.current - 1).min(self.max);
                Some((start, stop))
            } else {
                None
            }
        }
    }

    struct InfoFetcher {
        id: u64,
        num_len: usize,
        eps: Option<std::vec::IntoIter<Episode>>,
        pages: Option<Pages>,
        finish: bool,
    }

    impl Iterator for InfoFetcher {
        type Item = Result<Episode>;

        fn next(&mut self) -> Option<Self::Item> {
            loop {
                if let Some(mut eps) = self.eps.take() {
                    if let Some(mut ep) = eps.next() {
                        self.eps = Some(eps);
                        ep.number.pad_left(self.num_len);
                        return Some(Ok(ep));
                    }
                }

                if let Some(mut pages) = self.pages.take() {
                    if let Some((start, stop)) = pages.next() {
                        self.pages = Some(pages);
                        match fetch_info_page(self.id, start, stop) {
                            Ok(i) => {
                                self.eps = Some(i.episodes.into_iter());
                                continue;
                            }
                            Err(err) => {
                                self.finish = true;
                                return Some(Err(err));
                            }
                        }
                    } else {
                        self.finish = true;
                    }
                }

                if self.finish {
                    return None;
                }

                match fetch_info_page(self.id, 1, 120) {
                    Ok(info) => {
                        self.eps = Some(info.episodes.into_iter());
                        self.num_len = num_len(info.episodes_count);
                        let mut pages = Pages::new(info.episodes_count);
                        _ = pages.next();
                        self.pages = Some(pages);
                    }
                    Err(err) => {
                        self.finish = true;
                        return Some(Err(err));
                    }
                };
            }
        }
    }

    InfoFetcher {
        id,
        num_len: 0,
        eps: None,
        pages: None,
        finish: false,
    }
}

#[derive(Debug, Deserialize)]
pub struct IdNumber {
    pub id: u64,
    pub number: u64,
}

#[derive(Debug, Deserialize)]
pub struct VideoCdn {
    pub id: u64,
    pub number: u64,
    pub r#type: String,
    pub proxies: Vec<IdNumber>,
}

#[derive(Debug, Deserialize)]
pub struct Video {
    pub id: u64,
    pub name: String,
    pub client_ip: String,
    pub folder_id: String,
    #[serde(deserialize_with = "u8_to_bool")]
    pub legacy: bool,
    pub quality: u16,
    pub storage: IdNumber,
    pub storage_download: IdNumber,
    pub host: String,
    pub proxy_index: u64,
    pub proxy_download: u64,
    pub cdn: VideoCdn,
    pub size: u64,
}

fn u8_to_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    u8::deserialize(deserializer).map(|n| n != 0)
}

fn parse_url(url: &str) -> Result<(u64, Option<u64>)> {
    let url = url::Url::parse(url).context("Invalid URL")?;

    {
        let is_valid_host = match url.host_str() {
            Some(host) => host == "animeunity.it" || host == "www.animeunity.it",
            _ => false,
        };
        if !is_valid_host {
            bail!("Invalid URL");
        }
    }

    'err: {
        match url.path_segments() {
            Some(mut segs) => {
                if !matches!(segs.next(), Some("anime")) {
                    break 'err;
                }

                let id = match segs.next() {
                    Some(slug) => {
                        if let Ok(id) = slug.split('-').next().unwrap().parse::<u64>() {
                            id
                        } else {
                            break 'err;
                        }
                    }
                    None => break 'err,
                };

                let ep = match segs.next() {
                    Some(e) => {
                        if segs.next().is_some() {
                            break 'err;
                        }
                        if let Ok(ep) = e.parse() {
                            Some(ep)
                        } else {
                            break 'err;
                        }
                    }
                    None => None,
                };

                return Ok((id, ep));
            }
            None => break 'err,
        }
    }

    bail!("Invalid URL")
}

fn fetch_episode(id: u64) -> Result<Video> {
    let url = format!("https://scws.work/videos/{}", id);

    let body = ureq::get(url.as_str())
        .call()
        .context("Invalid response")?
        .into_string()
        .context("Invalid response")?;
    serde_json::from_str(body.as_str()).context("Invalid video")
}

pub enum DownloadLink {
    Legacy(Url),
    Current(Url, String),
}

impl DownloadLink {
    pub fn download_link(self) -> Result<Url> {
        let u = ureq::get("https://au-a1-01.scws-content.net/get-ip")
            .call()
            .context("Cannot get IP")?
            .into_string()
            .context("Cannot get IP")?;

        let expires = {
            let x = SystemTime::now().duration_since(UNIX_EPOCH).unwrap()
                + Duration::from_millis(36_00000 * 2);
            ((x.as_secs() as u128) + ((x.subsec_millis() / 500) as u128)).to_string()
        };
        let mut token = base64::engine::general_purpose::STANDARD_NO_PAD
            .encode(md5::compute(format!("{}{} Yc8U6r8KjAKAepEA", expires, u)).as_slice());
        {
            let buf = unsafe { token.as_mut_vec() };
            let mut i = 0;
            while let Some(pos) = buf
                .as_slice()
                .get(i..)
                .and_then(|h| memchr::memchr2(b'+', b'/', h))
            {
                i += pos;

                match buf[i] {
                    b'+' => {
                        buf[i] = b'-';
                    }
                    b'/' => {
                        buf[i] = b'_';
                    }
                    _ => unreachable!(),
                }

                i += 1;
            }
        }

        let url = match self {
            Self::Legacy(mut url) => {
                {
                    let mut pairs = url.query_pairs_mut();
                    pairs.append_pair("token", &token);
                    pairs.append_pair("expires", &expires);
                }
                url
            }
            Self::Current(mut url, filename) => {
                {
                    let mut pairs = url.query_pairs_mut();
                    pairs.append_pair("token", &token);
                    pairs.append_pair("expires", &expires);
                    pairs.append_pair("filename", &filename);
                }
                url
            }
        };

        Ok(url)
    }
}

fn usage() {
    println!(
        "USAGE: {} [--<executor>] <URL>",
        std::env::args().next().unwrap()
    );
    let mut cfg = ProjectDirs::from("dev", "shurizzle", "AnimeUnity Downloader")
        .unwrap()
        .config_dir()
        .to_path_buf();
    cfg.push("config.yaml");
    println!("config: {}", cfg.display());
}

fn _main() -> Result<()> {
    let (url, ex) = match std::env::args().len() {
        2 => (std::env::args().nth(1).unwrap(), config::Executor::Print),
        3 => {
            let (mut e, mut url) = {
                let mut it = std::env::args().skip(1);
                (it.next().unwrap(), it.next().unwrap())
            };

            if url.starts_with("--") {
                std::mem::swap(&mut e, &mut url);
            }

            if e.starts_with("--") {
                e.remove(0);
                e.remove(0);
                let executor = if let Some(executor) =
                    config::load()?.remove(&e).map(config::Executor::Command)
                {
                    executor
                } else {
                    println!("Invalid executor {:?}", e);
                    std::process::exit(1);
                };

                (url, executor)
            } else {
                usage();
                std::process::exit(1);
            }
        }
        _ => {
            usage();
            std::process::exit(1);
        }
    };

    let (id, epno) = parse_url(&url)?;

    let mut defaults = Vec::new();
    let mut reprs = Vec::new();
    let mut data = Vec::new();

    for ep in fetch_info(id) {
        let Episode {
            number,
            id,
            scws_id,
            file_name,
        } = ep?;

        defaults.push(epno.map_or(true, |epno| id == epno));
        reprs.push(number);
        data.push((scws_id, file_name));
    }

    let selections = MultiSelect::with_theme(&ColorfulTheme::default())
        .items(reprs.as_slice())
        .defaults(defaults.as_slice())
        .max_length(120)
        .interact_opt()?;
    let mut selections = if let Some(s) = selections {
        s
    } else {
        return Ok(());
    };
    selections.sort_unstable();

    for (i, data) in data.into_iter().enumerate() {
        if selections.is_empty() {
            break;
        }

        match selections.binary_search(&i) {
            Ok(i) => {
                selections.remove(i);
            }
            Err(_) => continue,
        }

        let (scws_id, file_name) = data;
        let e = fetch_episode(scws_id)?;

        let (file, url) = {
            let expires = {
                let x = SystemTime::now().duration_since(UNIX_EPOCH).unwrap()
                    + Duration::from_millis(36_00000 * 2);
                ((x.as_secs() as u128) + ((x.subsec_millis() / 500) as u128)).to_string()
            };
            let mut token = base64::engine::general_purpose::STANDARD_NO_PAD.encode(
                md5::compute(format!("{}{} Yc8U6r8KjAKAepEA", expires, e.client_ip)).as_slice(),
            );
            {
                let buf = unsafe { token.as_mut_vec() };
                let mut i = 0;
                while let Some(pos) = buf
                    .as_slice()
                    .get(i..)
                    .and_then(|h| memchr::memchr2(b'+', b'/', h))
                {
                    i += pos;

                    match buf[i] {
                        b'+' => {
                            buf[i] = b'-';
                        }
                        b'/' => {
                            buf[i] = b'_';
                        }
                        _ => unreachable!(),
                    }

                    i += 1;
                }
            }

            if e.legacy {
                let mut url =
                    Url::parse(&format!("https://au-dl-1.scws-content.net/{}", file_name)).unwrap();
                {
                    let mut pairs = url.query_pairs_mut();
                    pairs.append_pair("id", &scws_id.to_string());
                    pairs.append_pair("f", &e.folder_id);
                    pairs.append_pair("s", &e.storage.number.to_string());
                    pairs.append_pair("token", &token);
                    pairs.append_pair("expires", &expires);
                }
                (file_name, url)
            } else {
                let mut url = Url::parse(&format!(
                    "https://au-d1-0{}.{}/download/{}/{}/{}p.mp4",
                    e.proxy_download, e.host, e.storage_download.number, e.folder_id, e.quality
                ))
                .unwrap();
                {
                    let mut pairs = url.query_pairs_mut();
                    pairs.append_pair("token", &token);
                    pairs.append_pair("expires", &expires);
                    pairs.append_pair("filename", &e.name.replace('&', "."));
                }
                (e.name, url)
            }
        };

        let mut values = HashMap::new();
        values.insert("url", url.to_string());
        values.insert("file", file);
        ex.execute(&values)?;
    }

    Ok(())
}

fn main() {
    if let Err(err) = _main() {
        println!("{}", err);
        std::process::exit(1);
    }
}
