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
use urlencoding::Encoded;

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

impl Requirements {
    pub fn needs_title(&self) -> bool {
        !(*self & (Self::TITLE | Self::MAL_ID | Self::ANILIST_ID)).is_empty()
    }
}

impl AnimeContext {
    fn fetch_title(&mut self) -> Result<()> {
        let url = format!(
            "https://www.animeunity.so/anime/{}-{}",
            self.anime_id,
            self.slug
                .as_ref()
                .ok_or_else(|| anyhow!("cannot find slug"))?
        );

        let body = http::get(&url).context("Invalid informations")?;

        if let Some(anime) = dom::html_first(
            body.as_bytes(),
            dom::filter_tag_attr("video-player", "anime"),
        ) {
            #[derive(Debug, Deserialize)]
            struct Info {
                pub title_eng: Box<str>,
            }
            let Info { title_eng: title } =
                serde_json::from_slice(anime.as_bytes()).context("Invalid player informations")?;
            self.title = Some(title);
            return Ok(());
        }

        bail!("Cannot find anime title");
    }

    fn fetch_ids<F>(&mut self, mut f: F) -> Result<()>
    where
        F: FnMut(&mut AnimeContext) -> bool,
    {
        let url = format!(
            "https://www.animeunity.so/archivio/?title={}",
            Encoded(
                self.title
                    .as_ref()
                    .ok_or_else(|| anyhow!("cannot find title"))?
                    .as_bytes()
            )
        );

        let body = http::get(&url).context("Invalid informations")?;

        if let Some(anime) =
            dom::html_first(body.as_bytes(), dom::filter_tag_attr("archivio", "records"))
        {
            #[derive(Deserialize)]
            struct Info {
                pub id: u64,
                pub anilist_id: Option<u64>,
                pub mal_id: Option<u64>,
            }
            let infos: Vec<Info> =
                serde_json::from_slice(anime.as_bytes()).context("Invalid player informations")?;
            for Info {
                id,
                anilist_id,
                mal_id,
            } in infos
            {
                if id == self.anime_id {
                    if let Some(anilist_id) = anilist_id {
                        self.anilist_id = Some(anilist_id);
                    }
                    if let Some(mal_id) = mal_id {
                        self.mal_id = Some(mal_id);
                    }
                    if f(self) {
                        return Ok(());
                    }
                }
            }
        }

        Ok(())
    }

    pub fn fetch_requirements(&mut self, reqs: Requirements) -> Result<()> {
        if reqs.needs_title() {
            self.fetch_title()?;
        }
        match (
            reqs.contains(Requirements::ANILIST_ID),
            reqs.contains(Requirements::MAL_ID),
        ) {
            (true, true) => {
                self.fetch_ids(|me| me.anilist_id.is_some() && me.mal_id.is_some())?;
                match (self.anilist_id.is_none(), self.mal_id.is_none()) {
                    (true, true) => Err(anyhow!("Cannot find anilist_id and mal_id")),
                    (false, true) => Err(anyhow!("Cannot find mal_id")),
                    (true, false) => Err(anyhow!("Cannot find anilist_id")),
                    (false, false) => Ok(()),
                }
            }
            (false, true) => {
                self.fetch_ids(|me| me.mal_id.is_some())?;
                if self.mal_id.is_none() {
                    Err(anyhow!("Cannot find mal_id"))
                } else {
                    Ok(())
                }
            }
            (true, false) => {
                self.fetch_ids(|me| me.anilist_id.is_some())?;
                if self.anilist_id.is_none() {
                    Err(anyhow!("Cannot find anilist_id"))
                } else {
                    Ok(())
                }
            }
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
        let is_valid_host = match url.host_str() {
            Some(host) => {
                host == "animeunity.to"
                    || host == "www.animeunity.to"
                    || host == "animeunity.so"
                    || host == "www.animeunity.so"
            }
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

fn _fetch_video_infos(id: u64) -> Result<RawVideo> {
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
        dom::html_filter(http::get(&fetch_embed_url(id)?)?.as_bytes(), filter_script)
            .map(|mut s| {
                s.trim_in_place();
                s
            })
            .filter(|s| !s.is_empty())
            .fold(
                String::from("const window=this||globalThis||{};"),
                |mut code, script| {
                    code.push_str("try{");
                    code.push_str(&script);
                    code.push_str("}catch(____e){}\n");
                    code
                },
            ),
    )
}

pub fn fetch_video_infos(id: u64) -> Result<Video> {
    let RawVideo { file, url } = _fetch_video_infos(id)?;

    let file = if file.is_none() {
        if let Ok(uri) = Url::parse(&url) {
            'file: {
                for (k, n) in uri.query_pairs() {
                    if k == "filename"
                        && let Some(n) = n.split('/').next_back()
                        && !n.is_empty()
                    {
                        break 'file Some(n.to_string().into_boxed_str());
                    }
                }
                None
            }
        } else {
            None
        }
    } else {
        file
    };

    // TODO: check Content-Disposition

    if let Some(file) = file {
        Ok(Video { file, url })
    } else {
        bail!("file not found")
    }
}

fn fetch_embed_url(id: u64) -> Result<String> {
    http::get(&format!("https://www.animeunity.so/embed-url/{id}"))
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

#[derive(Debug, Deserialize)]
pub struct Info {
    pub slug: Option<Box<str>>,
    pub title: Option<Box<str>>,
    pub episodes_count: u64,
    pub episodes: Vec<Episode>,
}

pub fn fetch_info<'a>(
    id: u64,
    slug: &'a mut Option<Box<str>>,
    title: &'a mut Option<Box<str>>,
) -> impl Iterator<Item = Result<(Box<str>, Episode)>> + 'a {
    #[derive(Deserialize)]
    pub struct InfoMin {
        pub episodes_count: u64,
        pub episodes: Vec<Episode>,
    }

    #[derive(Deserialize)]
    pub struct InfoSlug {
        pub slug: Option<Box<str>>,
        pub episodes_count: u64,
        pub episodes: Vec<Episode>,
    }

    #[derive(Deserialize)]
    pub struct InfoTitle {
        pub name: Option<Box<str>>,
        pub episodes_count: u64,
        pub episodes: Vec<Episode>,
    }

    #[derive(Deserialize)]
    pub struct InfoSlugTitle {
        pub name: Option<Box<str>>,
        pub slug: Option<Box<str>>,
        pub episodes_count: u64,
        pub episodes: Vec<Episode>,
    }

    impl From<InfoMin> for Info {
        fn from(
            InfoMin {
                episodes_count,
                episodes,
            }: InfoMin,
        ) -> Self {
            Info {
                slug: None,
                title: None,
                episodes_count,
                episodes,
            }
        }
    }

    impl From<InfoSlug> for Info {
        fn from(
            InfoSlug {
                slug,
                episodes_count,
                episodes,
            }: InfoSlug,
        ) -> Self {
            Info {
                slug,
                title: None,
                episodes_count,
                episodes,
            }
        }
    }

    impl From<InfoTitle> for Info {
        fn from(
            InfoTitle {
                name,
                episodes_count,
                episodes,
            }: InfoTitle,
        ) -> Self {
            Info {
                slug: None,
                title: name,
                episodes_count,
                episodes,
            }
        }
    }

    impl From<InfoSlugTitle> for Info {
        fn from(
            InfoSlugTitle {
                slug,
                name,
                episodes_count,
                episodes,
            }: InfoSlugTitle,
        ) -> Self {
            Info {
                slug,
                title: name,
                episodes_count,
                episodes,
            }
        }
    }

    fn parse_info<'a, T: Into<Info> + Deserialize<'a>>(body: &'a str) -> serde_json::Result<Info> {
        serde_json::from_slice::<T>(body.as_bytes()).map(Into::into)
    }

    fn fetch_info_page<'a>(
        id: u64,
        start: u64,
        stop: u64,
        slug: &'a mut Option<Box<str>>,
        title: &'a mut Option<Box<str>>,
    ) -> Result<Info> {
        let url = format!(
            "https://www.animeunity.so/info_api/{}/1?start_range={}&end_range={}",
            id, start, stop
        );

        let body = http::get(&url).context("Invalid informations")?;

        match (slug.is_none(), title.is_none()) {
            (true, true) => parse_info::<InfoSlugTitle>(&body),
            (true, false) => parse_info::<InfoSlug>(&body),
            (false, true) => parse_info::<InfoTitle>(&body),
            (false, false) => parse_info::<InfoMin>(&body),
        }
        .context("Invalid informations")
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

    struct InfoFetcher<'a> {
        id: u64,
        num_len: usize,
        eps: Option<std::vec::IntoIter<Episode>>,
        pages: Option<Pages>,
        slug: &'a mut Option<Box<str>>,
        title: &'a mut Option<Box<str>>,
        finish: bool,
    }

    impl<'a> Iterator for InfoFetcher<'a> {
        type Item = Result<(Box<str>, Episode)>;

        fn next(&mut self) -> Option<Self::Item> {
            loop {
                if let Some(mut eps) = self.eps.take()
                    && let Some(ep) = eps.next()
                {
                    self.eps = Some(eps);
                    let mut name = ep.number.clone();
                    name.pad_left(self.num_len);
                    return Some(Ok((name.into(), ep)));
                }

                if let Some(mut pages) = self.pages.take() {
                    if let Some((start, stop)) = pages.next() {
                        self.pages = Some(pages);
                        match fetch_info_page(self.id, start, stop, self.slug, self.title) {
                            Ok(mut i) => {
                                if let Some(slug) = i.slug.take() {
                                    *self.slug = Some(slug);
                                }
                                if let Some(title) = i.title.take() {
                                    *self.title = Some(title);
                                }
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

                match fetch_info_page(self.id, 1, 120, self.slug, self.title) {
                    Ok(mut info) => {
                        if let Some(slug) = info.slug.take() {
                            *self.slug = Some(slug);
                        }
                        if let Some(title) = info.title.take() {
                            *self.title = Some(title);
                        }
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
        slug,
        title,
    }
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
