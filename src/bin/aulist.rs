use std::io::Write;

pub use audown::*;

use anyhow::{bail, Result};

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

    std::io::stdout().write_all(b"[")?;
    if let Some(ep) = anime.episode {
        let Video { url, .. } = fetch_video_infos(ep)?;
        serde_json::to_writer(std::io::stdout(), &url)?;
    } else {
        let mut eps = fetch_info(anime.anime_id, &mut anime.slug, &mut anime.title)
            .map(|res| res.map(|(_, ep)| ep.id))
            .collect::<Result<Vec<_>>>()?
            .into_iter();
        let Some(slug) = anime.slug.as_ref().map(|s| s.as_ref()) else {
            bail!("Cannot find slug");
        };

        if let Some(first) = eps.next() {
            let Video { url, .. } = fetch_video_infos(first)?;
            serde_json::to_writer(std::io::stdout(), &url)?;
        }
        for ep in eps {
            std::io::stdout().write_all(b",")?;
            serde_json::to_writer(
                std::io::stdout(),
                &format_args!(
                    "https://www.animeunity.to/anime/{}-{slug}/{ep}",
                    anime.anime_id
                ),
            )?;
        }
    }
    std::io::stdout().write_all(b"]")?;
    std::io::stdout().flush()?;

    Ok(())
}

fn main() {
    if let Err(err) = _main() {
        eprintln!("{}", err);
        std::process::exit(1);
    }
}
