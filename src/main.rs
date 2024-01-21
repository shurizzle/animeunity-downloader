mod config;
pub(crate) mod template;

use std::collections::HashMap;

use anyhow::{bail, Context, Result};
use dialoguer::{theme::ColorfulTheme, MultiSelect};
use directories::ProjectDirs;
use mini_v8::{FromValue, MiniV8};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Episode {
    pub id: u64,
    pub number: String,
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
            "https://www.animeunity.to/info_api/{}/1?start_range={}&end_range={}",
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

fn parse_url(url: &str) -> Result<(u64, Option<u64>)> {
    let url = url::Url::parse(url).context("Invalid URL")?;

    {
        let is_valid_host = match url.host_str() {
            Some(host) => host == "animeunity.to" || host == "www.animeunity.to",
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

fn fetch_embed_url(id: u64) -> Result<String> {
    Ok(
        ureq::get(&format!("https://www.animeunity.to/embed-url/{id}"))
            .call()?
            .into_string()?,
    )
}

#[derive(Debug, Clone, Deserialize)]
pub struct Video {
    pub file: String,
    pub url: String,
}

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
            .to_string();
        let url = value
            .get::<_, mini_v8::Value>("url")?
            .coerce_string(mv8)?
            .to_string();

        Ok(Self { file, url })
    }
}

fn extract_video_infos(code: String) -> Result<Video> {
    let mv8 = MiniV8::new();
    match mv8.eval(code) {
        Ok(()) => (),
        Err(err) => {
            bail!("{}", err)
        }
    }
    match mv8.eval::<_, Video>(
        "({file:window.video.filename||window.video.name,url:window.downloadUrl})",
    ) {
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

fn fetch_video_infos(id: u64) -> Result<Video> {
    let body = ureq::get(&fetch_embed_url(id)?).call()?.into_string()?;

    let script = {
        use soup::prelude::*;

        let soup = Soup::new(&body);
        let mut code = String::from("const window={};");
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
        let Episode { number, id } = ep?;

        defaults.push(epno.map_or(true, |epno| id == epno));
        reprs.push(number);
        data.push(id);
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

    for (i, id) in data.into_iter().enumerate() {
        if selections.is_empty() {
            break;
        }

        match selections.binary_search(&i) {
            Ok(i) => {
                selections.remove(i);
            }
            Err(_) => continue,
        }

        let Video { file, url } = fetch_video_infos(id)?;

        let mut values = HashMap::new();
        values.insert("url", url);
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
