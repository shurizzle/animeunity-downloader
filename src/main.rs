mod config;
pub(crate) mod template;

use std::fmt;

use anyhow::{anyhow, bail, Context, Result};
#[cfg(not(feature = "v8"))]
use boa_engine::{js_str, value::JsValue, Source};
use dialoguer::{theme::ColorfulTheme, MultiSelect};
use directories::ProjectDirs;
#[cfg(feature = "v8")]
use mini_v8::{FromValue, MiniV8};
use serde::Deserialize;
use template::Variables;
use urlencoding::Encoded;

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

#[derive(Debug, Clone, Deserialize)]
pub struct Video {
    pub file: Box<str>,
    pub url: Box<str>,
}

#[derive(Debug, Clone)]
pub struct EpisodeVariables<'a> {
    anime: &'a AnimeContext,
    video: &'a Video,
    episode: &'a Episode,
}

#[derive(Debug, Clone, Copy)]
pub enum EpisodeValue<'a> {
    Str(&'a str),
    U64(u64),
}

impl<'a> fmt::Display for EpisodeValue<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EpisodeValue::Str(s) => fmt::Display::fmt(s, f),
            EpisodeValue::U64(s) => fmt::Display::fmt(s, f),
        }
    }
}

impl<'a> EpisodeVariables<'a> {
    #[inline]
    pub fn new(anime: &'a AnimeContext, video: &'a Video, episode: &'a Episode) -> Self {
        Self {
            anime,
            video,
            episode,
        }
    }
}

impl<'a> Variables for EpisodeVariables<'a> {
    type Item<'b> = EpisodeValue<'b>
    where
        Self: 'b;

    #[allow(clippy::needless_lifetimes)]
    fn get<'b, S: AsRef<str>>(&'b self, name: S) -> Option<Self::Item<'b>> {
        let name = name.as_ref();
        match name {
            "slug" => self.anime.slug.as_deref().map(EpisodeValue::Str),
            "title" => self.anime.title.as_deref().map(EpisodeValue::Str),
            "mal_id" => self.anime.mal_id.map(EpisodeValue::U64),
            "anilist_id" => self.anime.anilist_id.map(EpisodeValue::U64),
            "episode" => Some(EpisodeValue::Str(&self.episode.number)),
            "file" => Some(EpisodeValue::Str(&self.video.file)),
            "url" => Some(EpisodeValue::Str(&self.video.url)),
            _ => None,
        }
    }
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

fn fetch_info<'a>(
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
            "https://www.animeunity.to/info_api/{}/1?start_range={}&end_range={}",
            id, start, stop
        );

        let body = ureq::get(&url)
            .call()
            .context("Invalid informations")?
            .into_string()
            .context("Invalid informations")?;

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
                if let Some(mut eps) = self.eps.take() {
                    if let Some(ep) = eps.next() {
                        self.eps = Some(eps);
                        let mut name = ep.number.clone();
                        name.pad_left(self.num_len);
                        return Some(Ok((name.into(), ep)));
                    }
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

fn parse_url(url: &str) -> Result<AnimeContext> {
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
            Some(host) => host == "animeunity.to" || host == "www.animeunity.to",
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

fn fetch_embed_url(id: u64) -> Result<String> {
    Ok(
        ureq::get(&format!("https://www.animeunity.to/embed-url/{id}"))
            .call()?
            .into_string()?,
    )
}

#[cfg(feature = "v8")]
pub fn type_name(value: &mini_v8::Value) -> &'static str {
    use mini_v8::Value;
    match value {
        Value::Undefined => "undefined",
        Value::Null => "null",
        Value::Boolean(_) => "boolean",
        Value::Number(_) => "number",
        Value::Date(_) => "date",
        Value::Function(_) => "function",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        Value::String(_) => "string",
    }
}

#[cfg(feature = "v8")]
impl FromValue for Video {
    fn from_value(value: mini_v8::Value, mv8: &MiniV8) -> mini_v8::Result<Self> {
        let value = match value {
            mini_v8::Value::Object(v) => v,
            _ => {
                return Err(mini_v8::Error::FromJsConversionError {
                    from: type_name(&value),
                    to: "Video",
                })
            }
        };

        let file = value
            .get::<_, mini_v8::Value>("file")?
            .coerce_string(mv8)?
            .to_string()
            .into_boxed_str();
        let url = value
            .get::<_, mini_v8::Value>("url")?
            .coerce_string(mv8)?
            .to_string()
            .into_boxed_str();

        Ok(Self { file, url })
    }
}

#[cfg(feature = "v8")]
fn extract_video_infos(mut code: String) -> Result<Video> {
    let mv8 = MiniV8::new();
    code.push_str("\n({file:window.video.filename||window.video.name,url:window.downloadUrl})");
    match mv8.eval::<_, Video>(code) {
        Ok(x) => {
            if x.url.is_empty() {
                bail!("url not found");
            }
            if x.file.is_empty() {
                bail!("file not found");
            }
            Ok(x)
        }
        Err(err) => {
            bail!("{}", err)
        }
    }
}

#[cfg(not(feature = "v8"))]
fn extract_video_infos(mut code: String) -> Result<Video> {
    let mut ctx = boa_engine::Context::default();
    code.push_str("\n({file:window.video.filename||window.video.name,url:window.downloadUrl})");
    println!("{code}");
    match ctx
        .eval(Source::from_bytes(&code))
        .map_err(|e| anyhow!("{e}"))?
    {
        JsValue::Object(o) => {
            let url = match o
                .get(js_str!("url"), &mut ctx)
                .map_err(|e| anyhow!("{e}"))?
            {
                JsValue::String(s) => s.to_std_string()?.into_boxed_str(),
                _ => bail!("url not found"),
            };
            if url.is_empty() {
                bail!("url not found");
            }

            let file = match o
                .get(js_str!("file"), &mut ctx)
                .map_err(|e| anyhow!("{e}"))?
            {
                JsValue::String(s) => s.to_std_string()?.into_boxed_str(),
                _ => bail!("file not found"),
            };
            if file.is_empty() {
                bail!("file not found");
            }

            Ok(Video { file, url })
        }
        _ => unreachable!(),
    }
}

fn fetch_video_infos(id: u64) -> Result<Video> {
    let body = ureq::get(&fetch_embed_url(id)?).call()?.into_string()?;

    let script = {
        use soup::prelude::*;

        let soup = Soup::new(&body);
        let mut code = String::from("const window=this||{};");
        for script in soup.tag("script").find_all() {
            if script.get("src").is_none() {
                let text = script.text();
                let text = text.trim();
                code.push_str("try{");
                code.push_str(text);
                code.push_str("}catch(____e){}");
                code.push('\n');
            }
        }
        code
    };

    extract_video_infos(script)
}

impl AnimeContext {
    fn fetch_title(&mut self) -> Result<()> {
        let url = format!(
            "https://www.animeunity.to/anime/{}-{}",
            self.anime_id,
            self.slug
                .as_ref()
                .ok_or_else(|| anyhow!("cannot find slug"))?
        );

        let body = ureq::get(&url)
            .call()
            .context("Invalid informations")?
            .into_string()
            .context("Invalid informations")?;

        {
            use soup::prelude::*;

            let soup = Soup::new(&body);
            for player in soup.tag("video-player").find_all() {
                #[derive(Debug, Deserialize)]
                struct Info {
                    pub title_eng: Box<str>,
                }
                if let Some(anime) = player.get("anime") {
                    let Info { title_eng: title } = serde_json::from_slice(anime.as_bytes())
                        .context("Invalid player informations")?;
                    self.title = Some(title);
                    return Ok(());
                }
            }
        }

        bail!("Cannot find anime title");
    }

    fn fetch_ids<F>(&mut self, mut f: F) -> Result<()>
    where
        F: FnMut(&mut AnimeContext) -> bool,
    {
        let url = format!(
            "https://www.animeunity.to/archivio/?title={}",
            Encoded(
                self.title
                    .as_ref()
                    .ok_or_else(|| anyhow!("cannot find title"))?
                    .as_bytes()
            )
        );

        let body = ureq::get(&url)
            .call()
            .context("Invalid informations")?
            .into_string()
            .context("Invalid informations")?;

        {
            use soup::prelude::*;

            let soup = Soup::new(&body);
            for player in soup.tag("archivio").find_all() {
                #[derive(Deserialize)]
                struct Info {
                    pub id: u64,
                    pub anilist_id: Option<u64>,
                    pub mal_id: Option<u64>,
                }
                if let Some(anime) = player.get("records") {
                    let infos: Vec<Info> = serde_json::from_slice(anime.as_bytes())
                        .context("Invalid player informations")?;
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

fn usage() {
    println!(
        "USAGE: {} [--<executor>] <URL|ID>",
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

    let mut anime = parse_url(&url)?;

    let mut defaults = Vec::new();
    let mut reprs = Vec::new();
    let mut data = Vec::new();

    for ep in fetch_info(anime.anime_id, &mut anime.slug, &mut anime.title) {
        let (no, episode) = ep?;

        defaults.push(anime.episode.map_or(true, |epno| episode.id == epno));
        reprs.push(no);
        data.push(episode);
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

    let mut reqs = Requirements::empty();
    for v in ex.variables() {
        match v {
            "mal_id" => reqs |= Requirements::MAL_ID,
            "anilist_id" => reqs |= Requirements::ANILIST_ID,
            "title" => reqs |= Requirements::TITLE,
            _ => (),
        }
        if reqs.is_all() {
            break;
        }
    }
    if let Err(err) = anime.fetch_requirements(reqs) {
        eprintln!("{err}");
    }

    for (i, episode) in data.into_iter().enumerate() {
        if selections.is_empty() {
            break;
        }

        match selections.binary_search(&i) {
            Ok(i) => {
                selections.remove(i);
            }
            Err(_) => continue,
        }

        let video = fetch_video_infos(episode.id)?;

        ex.execute(&EpisodeVariables::new(&anime, &video, &episode))?;
    }

    Ok(())
}

fn main() {
    if let Err(err) = _main() {
        println!("{}", err);
        std::process::exit(1);
    }
}
