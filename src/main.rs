mod config;
pub use audown::*;

use std::fmt;

use anyhow::Result;
use dialoguer::{MultiSelect, theme::ColorfulTheme};
use directories::ProjectDirs;
use template::Variables;

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
    type Item<'b>
        = EpisodeValue<'b>
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

fn load_executor(name: Option<&str>) -> Result<config::Executor> {
    let Some(name) = name else {
        return Ok(config::load()?
            .remove("default")
            .map(config::Executor::Command)
            .unwrap_or(config::Executor::Print));
    };

    if let Some(executor) = config::load()?.remove(name).map(config::Executor::Command) {
        Ok(executor)
    } else {
        println!("Invalid executor {:?}", name);
        std::process::exit(1);
    }
}

fn _main() -> Result<()> {
    let (url, ex) = match std::env::args().len() {
        2 => (std::env::args().nth(1).unwrap(), load_executor(None)?),
        3 => {
            let (mut e, mut url) = {
                let mut it = std::env::args().skip(1);
                (it.next().unwrap(), it.next().unwrap())
            };

            if url.starts_with("--") {
                std::mem::swap(&mut e, &mut url);
            }

            if let Some(e) = e.strip_prefix("--") {
                let executor = load_executor(if e == "default" { None } else { Some(e) })?;

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

        defaults.push(anime.episode.is_none_or(|epno| episode.id == epno));
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
        eprintln!("{}", err);
        std::process::exit(1);
    }
}
