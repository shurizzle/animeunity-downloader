use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use htmlentity::entity::ICodedDataTrait;
use serde::{Deserialize, Deserializer};
use url::Url;

#[derive(Debug, Deserialize)]
pub struct Episode {
    pub id: u64,
    pub anime_id: u64,
    #[serde(deserialize_with = "str_to_u64")]
    pub number: u64,
    pub link: String,
    pub scws_id: u64,
    pub file_name: String,
}

#[derive(Debug, Deserialize)]
pub struct VideoStorage {
    pub id: u64,
    pub number: u64,
}

#[derive(Debug, Deserialize)]
pub struct VideoStorageDownload {
    pub id: u64,
    pub number: u64,
}

#[derive(Debug, Deserialize)]
pub struct VideoCdnProxy {
    pub id: u64,
    pub number: u64,
}

#[derive(Debug, Deserialize)]
pub struct VideoCdn {
    pub id: u64,
    pub number: u64,
    pub r#type: String,
    pub proxies: Vec<VideoCdnProxy>,
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
    pub storage: VideoStorage,
    pub storage_download: VideoStorageDownload,
    pub host: String,
    pub proxy_index: u64,
    pub proxy_download: u64,
    pub cdn: VideoCdn,
    pub size: u64,
}

fn str_to_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer)?
        .parse()
        .map_err(serde::de::Error::custom)
}

fn u8_to_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    u8::deserialize(deserializer).map(|n| n != 0)
}

fn parse_url(url: &str) -> Result<(Url, Option<u64>)> {
    let mut url = url::Url::parse(url).context("Invalid URL")?;
    {
        let is_valid_host = match url.host_str() {
            Some(host) => host == "animeunity.tv" || host == "www.animeunity.tv",
            _ => false,
        };
        if !is_valid_host {
            bail!("Invalid URL");
        }
    }
    let ep = {
        let mut ep = None;
        let is_valid_path = match url.path_segments() {
            Some(mut segs) => 'path: {
                if !matches!(segs.next(), Some("anime")) {
                    break 'path false;
                }

                if segs.next().is_none() {
                    break 'path false;
                }

                match segs.next() {
                    Some(e) => {
                        ep = Some(e.parse().context("Invalid URL")?);
                        segs.next().is_none()
                    }
                    None => true,
                }
            }
            _ => false,
        };
        if !is_valid_path {
            bail!("Invalid URL");
        }
        ep
    };

    if url.scheme() == "http" {
        url.set_scheme("https").unwrap();
    }

    if url.scheme() != "https" {
        bail!("Invalid URL");
    }
    Ok((url, ep))
}

fn fetch_anime(url: &Url) -> Result<Vec<Episode>> {
    let body = ureq::get(url.as_str())
        .call()
        .context("Invalid response")?
        .into_string()
        .context("Invalid response")?;
    let dom = tl::parse(&body, tl::ParserOptions::default()).context("Invalid page")?;
    let parser = dom.parser();
    let mut episodes = None;
    for node in dom.query_selector("video-player").unwrap() {
        let node = if let Some(node) = node.get(parser) {
            if let Some(node) = node.as_tag() {
                node
            } else {
                continue;
            }
        } else {
            continue;
        };

        let eps = if let Some(a) = node.attributes().get("episodes").flatten() {
            htmlentity::entity::decode(a.as_bytes()).to_bytes()
        } else {
            continue;
        };

        if episodes.is_some() {
            bail!("Invalid anime");
        }

        let mut eps: Vec<Episode> =
            serde_json::from_slice(eps.as_slice()).context("Invalid anime")?;
        eps.sort_unstable_by_key(|x| x.number);
        episodes = Some(eps);
    }
    episodes.ok_or_else(|| anyhow!("Invalid anime"))
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

fn _main() -> Result<()> {
    let (url, epno) =
        parse_url("https://www.animeunity.tv/anime/4417-lamu-urusei-yatsura-2022-ita")?;
    let mut def = None;
    let episodes = fetch_anime(&url)?
        .into_iter()
        .enumerate()
        .map(|(i, ep)| {
            if epno.map_or(false, |epno| ep.id == epno) {
                def = Some(i);
            }
            let e = fetch_episode(ep.scws_id)?;

            let repr = format!("{} - {}", ep.number, ep.file_name);
            let (file, link) = if e.legacy {
                let mut url = Url::parse(&format!(
                    "https://au-dl-1.scws-content.net/{}",
                    ep.file_name
                ))
                .unwrap();
                {
                    let mut pairs = url.query_pairs_mut();
                    pairs.append_pair("id", &ep.scws_id.to_string());
                    pairs.append_pair("f", &e.folder_id);
                    pairs.append_pair("s", &e.storage.number.to_string());
                }
                (ep.file_name, DownloadLink::Legacy(url))
            } else {
                let url = Url::parse(&format!(
                    "https://au-d1-0{}.{}/download/{}/{}/{}p.mp4",
                    e.proxy_download, e.host, e.storage_download.number, e.folder_id, e.quality
                ))
                .unwrap();
                let n = e.name.replace('&', ".");
                (e.name, DownloadLink::Current(url, n))
            };

            Ok((repr, file, link))
        })
        .collect::<Result<Vec<(String, String, DownloadLink)>>>()?;

    for (_, _, url) in episodes {
        println!("{}", url.download_link()?);
    }

    Ok(())
}

fn main() {
    if let Err(err) = _main() {
        println!("{}", err);
        std::process::exit(1);
    }
}
