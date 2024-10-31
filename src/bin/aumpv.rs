use std::io::Write;

pub use audown::*;

use anyhow::{bail, Result};

#[derive(Debug)]
pub struct Url<'a> {
    pub anime_id: u64,
    pub slug: &'a str,
    pub ep: u64,
}

impl<'se> serde::Serialize for Url<'se> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serde::Serialize::serialize::<S>(
            &format_args!(
                "https://www.animeunity.to/anime/{}-{}/{}",
                self.anime_id, self.slug, self.ep,
            ),
            serializer,
        )
    }
}

fn usage() {
    let name = std::env::args().next();
    let name = if let Some(ref name) = name {
        name
    } else {
        "aulist"
    };

    println!("USAGE: {name} <URL|ID>");
}

fn _main() -> Result<()> {
    let url = match std::env::args().len() {
        2 => std::env::args().nth(1).unwrap(),
        _ => {
            usage();
            std::process::exit(1);
        }
    };

    let mut anime = parse_url(&url)?;

    if let Some(ep) = anime.episode {
        let Video { url, .. } = fetch_video_infos(ep)?;
        std::io::stdout().write_all(b"{\"type\":\"video\",\"url\":")?;
        serde_json::to_writer(std::io::stdout(), &url)?;
        std::io::stdout().write_all(b"}")?;
    } else {
        let eps = fetch_info(anime.anime_id, &mut anime.slug, &mut anime.title)
            .map(|res| res.map(|(_, ep)| ep.id))
            .collect::<Result<Vec<_>>>()?;
        let Some(slug) = anime.slug.as_ref().map(|s| s.as_ref()) else {
            bail!("Cannot find slug");
        };
        let mut eps = eps.into_iter().map(|ep| Url {
            anime_id: anime.anime_id,
            slug,
            ep,
        });

        std::io::stdout().write_all(b"{\"type\":\"playlist\",\"items\":[")?;
        if let Some(ep) = eps.next() {
            serde_json::to_writer(std::io::stdout(), &ep)?;
        }
        for ep in eps {
            std::io::stdout().write_all(b",")?;
            serde_json::to_writer(std::io::stdout(), &ep)?;
        }
        std::io::stdout().write_all(b"]}")?;
    }
    std::io::stdout().flush()?;

    Ok(())
}

fn main() {
    if let Err(err) = _main() {
        eprintln!("{}", err);
        std::process::exit(1);
    }
}
