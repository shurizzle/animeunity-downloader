pub mod dom;
pub mod http;
pub mod js;
pub mod template;

use std::{borrow::Borrow, rc::Rc};

use anyhow::{Context, Result, anyhow, bail};
use markup5ever_rcdom::{Node, NodeData};
use serde::Deserialize;
use trim_in_place::TrimInPlace;
use url::Url;

#[macro_export]
macro_rules! domain {
    () => {
        "animeunity.cx"
    };
    (@map www_) => {
        $crate::domain!()
    };
    (@map _) => {
        $crate::domain!()
    };
    (@map $tt:tt) => {
        $tt
    };
    ($($tt:tt)*) => {
        concat!($($crate::domain!(@map $tt)),*)
    };
}

const VALID_DOMAINS: &[&str] = &[domain!(), "animeunity.co", "animeunity.so", "animeunity.to"];

#[derive(Debug, Clone, Deserialize)]
pub struct RawVideo {
    pub file: Option<Box<str>>,
    pub url: Box<str>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Video {
    pub file: Box<str>,
    pub url: Box<str>,
}

#[derive(Debug)]
pub struct AnimeContext {
    pub anime_id: u64,
    pub slug: Option<Box<str>>,
    pub title: Option<Box<str>>,
    pub episode: Option<u64>,
    pub mal_id: Option<u64>,
    pub anilist_id: Option<u64>,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct Requirements: u8 {
        const TITLE      = 1 << 0;
        const MAL_ID     = 1 << 1;
        const ANILIST_ID = 1 << 2;
    }
}

/// Inertia props embedded in the `data-page` attribute of a page.
fn fetch_page_props<T: serde::de::DeserializeOwned>(url: &str) -> Result<T> {
    #[derive(Deserialize)]
    struct Page<T> {
        props: T,
    }

    let body = http::get(url).context("Invalid informations")?;
    let page = dom::html_first(body.as_bytes(), dom::filter_tag_attr("div", "data-page"))
        .ok_or_else(|| anyhow!("Cannot find page informations"))?;
    Ok(serde_json::from_str::<Page<T>>(&page)
        .context("Invalid page informations")?
        .props)
}

fn anime_url(anime_id: u64, slug: Option<&str>) -> String {
    // shortcut: unknown slug relies on the server redirecting to the canonical one, fetch slug first if that stops working
    format!(
        domain!("https://" www_ "/anime/{}-{}"),
        anime_id,
        slug.unwrap_or("_")
    )
}

impl AnimeContext {
    fn fetch_anime(&mut self) -> Result<()> {
        #[derive(Deserialize)]
        struct Anime {
            slug: Box<str>,
            title: Box<str>,
            mal_id: Option<u64>,
            anilist_id: Option<u64>,
        }
        #[derive(Deserialize)]
        struct Props {
            anime: Anime,
        }

        let Props { anime } = fetch_page_props(&anime_url(self.anime_id, self.slug.as_deref()))?;
        self.slug.get_or_insert(anime.slug);
        self.title = Some(anime.title);
        self.mal_id = anime.mal_id;
        self.anilist_id = anime.anilist_id;
        Ok(())
    }

    pub fn fetch_requirements(&mut self, reqs: Requirements) -> Result<()> {
        if !reqs.is_empty() {
            self.fetch_anime()?;
        }
        match (
            reqs.contains(Requirements::ANILIST_ID) && self.anilist_id.is_none(),
            reqs.contains(Requirements::MAL_ID) && self.mal_id.is_none(),
        ) {
            (true, true) => Err(anyhow!("Cannot find anilist_id and mal_id")),
            (false, true) => Err(anyhow!("Cannot find mal_id")),
            (true, false) => Err(anyhow!("Cannot find anilist_id")),
            (false, false) => Ok(()),
        }
    }
}

pub fn parse_url(url: &str) -> Result<AnimeContext> {
    if let Ok(anime_id) = url.parse::<u64>() {
        return Ok(AnimeContext {
            anime_id,
            slug: None,
            title: None,
            episode: None,
            mal_id: None,
            anilist_id: None,
        });
    }

    let url = url::Url::parse(url).context("Invalid URL")?;

    {
        fn strip_www(host: &str) -> &str {
            host.strip_prefix("www.").unwrap_or(host)
        }

        let is_valid_host = match url.host_str() {
            Some(host) => VALID_DOMAINS.contains(&strip_www(host)),
            _ => false,
        };
        if !is_valid_host {
            bail!("Invalid domain");
        }
    }

    'err: {
        match url.path_segments() {
            Some(mut segs) => {
                if !matches!(segs.next(), Some("anime")) {
                    break 'err;
                }

                let (anime_id, slug) = match segs.next() {
                    Some(slug) => {
                        let mut it = slug.splitn(2, '-');
                        if let Some(id) = it.next().and_then(|id| id.parse::<u64>().ok()) {
                            let slug = it.next().and_then(|slug| {
                                if slug.is_empty() {
                                    None
                                } else {
                                    Some(slug.into())
                                }
                            });
                            (id, slug)
                        } else {
                            break 'err;
                        }
                    }
                    None => break 'err,
                };

                let episode = match segs.next() {
                    None => None,
                    Some(e) => {
                        if segs.next().is_some() {
                            break 'err;
                        }
                        if e.is_empty() {
                            None
                        } else if let Ok(ep) = e.parse() {
                            Some(ep)
                        } else {
                            break 'err;
                        }
                    }
                };

                return Ok(AnimeContext {
                    anime_id,
                    slug,
                    title: None,
                    episode,
                    mal_id: None,
                    anilist_id: None,
                });
            }
            None => break 'err,
        }
    }

    bail!("Invalid path")
}

fn _fetch_video_infos(anime_id: u64, slug: Option<&str>, id: u64) -> Result<RawVideo> {
    fn filter_script(node: Rc<Node>) -> Result<String, Rc<Node>> {
        match node.data {
            NodeData::Element {
                ref name,
                ref attrs,
                ..
            } => {
                if name.borrow().local.as_bytes() != b"script" {
                    return Err(node);
                }
                if attrs
                    .borrow()
                    .iter()
                    .any(|a| a.name.local.as_bytes() == b"src")
                {
                    return Err(node);
                }
                Ok(extract_text(node))
            }
            _ => Err(node),
        }
    }

    js::extract_video_infos(
        dom::html_filter(http::get(&fetch_embed_url(anime_id, slug, id)?)?.as_bytes(), filter_script)
            .map(|mut s| {
                s.trim_in_place();
                s
            })
            .filter(|s| !s.is_empty())
            .fold(
                String::from("const window=this||globalThis||{};function setTimeout(){};window.setTimeout=setTimeout;"),
                |mut code, script| {
                    code.push_str("try{");
                    code.push_str(&script);
                    code.push_str("}catch{}\n");
                    code
                },
            ),
    )
}

pub fn fetch_video_infos(anime_id: u64, slug: Option<&str>, id: u64) -> Result<Video> {
    let RawVideo { file, url } = _fetch_video_infos(anime_id, slug, id)?;

    let file = file.or_else(|| {
        Url::parse(&url).ok().and_then(|uri| {
            uri.query_pairs()
                .find(|(k, _)| k == "filename")
                .map(|(_, n)| n.into_owned().into_boxed_str())
        })
    });

    // TODO: check Content-Disposition

    if let Some(file) = file {
        let file = sanitize_filename::sanitize(&*file);
        if file.is_empty() {
            bail!("file not found")
        }
        Ok(Video {
            file: file.into_boxed_str(),
            url,
        })
    } else {
        bail!("file not found")
    }
}

fn fetch_embed_url(anime_id: u64, slug: Option<&str>, id: u64) -> Result<String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Props {
        embed_url: String,
    }

    Ok(fetch_page_props::<Props>(&format!("{}/{}", anime_url(anime_id, slug), id))?.embed_url)
}

fn extract_text(node: Rc<Node>) -> String {
    let mut acc = String::new();
    for content in dom::DomIterator::new(node, |node: Rc<Node>| {
        if let NodeData::Text { ref contents } = node.data {
            Ok(contents.take())
        } else {
            Err(node)
        }
    }) {
        if acc.is_empty() {
            acc = content.to_string();
        } else {
            acc.push_str(&content);
        }
    }
    acc
}

#[derive(Debug, Deserialize)]
pub struct Episode {
    pub id: u64,
    pub number: String,
}

/// Lists episodes, picking for each one the video in the language of `video`
/// (falls back to `ita`, then to any available language).
pub fn fetch_info(anime_id: u64, video: Option<u64>) -> Result<Vec<(Box<str>, Episode)>> {
    #[derive(Deserialize)]
    struct RawVideo {
        id: u64,
        language: Box<str>,
    }
    #[derive(Deserialize)]
    struct RawEpisode {
        number: String,
        videos: Vec<RawVideo>,
    }
    #[derive(Deserialize)]
    struct Page {
        episodes: Vec<RawEpisode>,
        tab_size: u64,
        total: u64,
    }

    let mut raw = Vec::new();
    let mut start = 1;
    loop {
        let page: Page = serde_json::from_str(
            &http::get(&format!(
                domain!("https://" www_ "/api/anime/{}/episodes?start={}"),
                anime_id, start
            ))
            .context("Invalid informations")?,
        )
        .context("Invalid informations")?;
        let empty = page.episodes.is_empty();
        raw.extend(page.episodes);
        start += page.tab_size;
        if empty || start > page.total {
            break;
        }
    }

    let lang: Box<str> = video
        .and_then(|v| raw.iter().flat_map(|e| &e.videos).find(|x| x.id == v))
        .map_or("ita".into(), |x| x.language.clone());

    let num_len = raw.iter().map(|e| e.number.len()).max().unwrap_or(0);
    Ok(raw
        .into_iter()
        .filter_map(|e| {
            let v = e
                .videos
                .iter()
                .find(|v| v.language == lang)
                .or_else(|| e.videos.iter().find(|v| &*v.language == "ita"))
                .or_else(|| e.videos.first())?;
            let mut name = e.number.clone();
            name.pad_left(num_len);
            Some((
                name.into(),
                Episode {
                    id: v.id,
                    number: e.number,
                },
            ))
        })
        .collect())
}

trait PadLeft {
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
