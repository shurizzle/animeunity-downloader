use std::{collections::HashMap, fs::File, process::Command};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{de::Error, Deserialize};

use crate::template::{Template, VarIter, Variables};

#[derive(Debug)]
pub enum Executor {
    Command(CommandExecutor),
    Print,
}

pub struct CommandVariables<'a> {
    inner: Option<VarIter<'a>>,
    args: std::slice::Iter<'a, Template>,
}

#[allow(dead_code)]
pub enum ExecutorVariables<'a> {
    Command(CommandVariables<'a>),
    Print(Option<&'a str>),
}

impl From<CommandExecutor> for Executor {
    #[inline]
    fn from(cmd: CommandExecutor) -> Self {
        Self::Command(cmd)
    }
}

impl<'a> From<CommandVariables<'a>> for ExecutorVariables<'a> {
    fn from(value: CommandVariables<'a>) -> Self {
        Self::Command(value)
    }
}

impl Executor {
    pub fn execute<V: Variables>(&self, values: &V) -> Result<()> {
        match self {
            Self::Command(cmd) => cmd.execute(values),
            Self::Print => {
                if let Some(url) = values.get("url") {
                    println!("{}", url);
                } else {
                    println!();
                }
                Ok(())
            }
        }
    }

    pub fn variables(&self) -> ExecutorVariables {
        match self {
            Executor::Command(cmd) => cmd.variables().into(),
            Executor::Print => ExecutorVariables::Print(Some("url")),
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
            Ok(Self(
                res.into_iter()
                    .map(Template::parse)
                    .collect::<Option<Vec<_>>>()
                    .and_then(|vec| if vec.is_empty() { None } else { Some(vec) })
                    .ok_or_else(|| D::Error::custom("Invalid command"))?,
            ))
        }
    }
}

impl<'a> Iterator for CommandVariables<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(inner) = self.inner.as_mut() {
                if let Some(name) = inner.next() {
                    return Some(name);
                } else {
                    self.inner = None;
                }
            }

            self.inner = Some(self.args.next()?.variables());
        }
    }
}

impl<'a> Iterator for ExecutorVariables<'a> {
    type Item = &'a str;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Command(it) => it.next(),
            Self::Print(it) => it.take(),
        }
    }
}

impl CommandExecutor {
    pub fn execute<V: Variables>(&self, values: &V) -> Result<()> {
        let mut cmd = Command::new(&*self.0[0].render(values));

        for x in self.0.iter().skip(1) {
            cmd.arg(&*x.render(values));
        }

        cmd.spawn()?.wait()?;
        Ok(())
    }

    pub fn variables(&self) -> CommandVariables {
        CommandVariables {
            inner: None,
            args: self.0.iter(),
        }
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
