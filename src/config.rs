use std::{collections::HashMap, fs::File, process::Command};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use new_string_template::template::Template;
use serde::{de::Error, Deserialize};

#[derive(Debug)]
pub enum Executor {
    Command(CommandExecutor),
    Print,
}

impl From<CommandExecutor> for Executor {
    #[inline]
    fn from(cmd: CommandExecutor) -> Self {
        Self::Command(cmd)
    }
}

impl Executor {
    pub fn execute<S: AsRef<str>>(&self, values: &HashMap<&str, S>) -> Result<()> {
        match self {
            Self::Command(cmd) => cmd.execute(values),
            Self::Print => {
                println!("{}", values.get("url").unwrap().as_ref());
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommandExecutor(Vec<Template>);

impl<'de> Deserialize<'de> for CommandExecutor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let res = <Vec<String> as Deserialize<'de>>::deserialize(deserializer)?;

        if res.is_empty() {
            Err(D::Error::custom("Invalid command"))
        } else {
            Ok(Self(res.into_iter().map(Template::new).collect()))
        }
    }
}

impl CommandExecutor {
    pub fn execute<S: AsRef<str>>(&self, values: &HashMap<&str, S>) -> Result<()> {
        let mut cmd = Command::new(&self.0[0].render(values)?);

        for x in self.0.iter().skip(1) {
            cmd.arg(x.render(values)?);
        }

        cmd.spawn()?.wait()?;
        Ok(())
    }
}

pub fn load() -> Result<HashMap<String, CommandExecutor>> {
    if let Some(prj_dirs) = ProjectDirs::from("dev", "shurizzle", "AnimeUnity Downloader") {
        let mut cfg = prj_dirs.config_dir().to_path_buf();
        cfg.push("config.yaml");

        if cfg.exists() {
            return serde_yaml::from_reader::<_, HashMap<String, CommandExecutor>>(
                File::open(cfg).context("Error while loading configuration")?,
            )
            .context("Error in configuration file");
        }
    }

    Ok(HashMap::new())
}
