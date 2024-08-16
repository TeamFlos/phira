//! Directory helper

use anyhow::{bail, Result};
use std::{
    fs::{File, ReadDir},
    path::{Component, Path, PathBuf},
};

pub struct Dir(PathBuf);

impl Dir {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if !path.is_dir() {
            bail!("not dir")
        }
        Ok(Self(path))
    }

    pub fn join(&self, path: impl AsRef<Path>) -> Result<PathBuf> {
        let path = path.as_ref();
        let mut res = self.0.clone();
        let mut depth = 0;
        for comp in path.components() {
            match comp {
                Component::Prefix(_) => {
                    bail!("prefix inside dir");
                }
                Component::ParentDir => {
                    if depth == 0 {
                        bail!("path traversal");
                    }
                    res.pop();
                    depth -= 1;
                }
                Component::Normal(name) => {
                    res.push(name);
                    depth += 1;
                }
                Component::RootDir | Component::CurDir => {}
            }
        }
        Ok(res)
    }

    #[inline]
    pub fn create_dir_all(&self, p: impl AsRef<Path>) -> Result<()> {
        std::fs::create_dir_all(self.join(p)?)?;
        Ok(())
    }

    #[inline]
    pub fn remove_dir_all(&self, p: impl AsRef<Path>) -> Result<()> {
        std::fs::remove_dir_all(self.join(p)?)?;
        Ok(())
    }

    #[inline]
    pub fn open_dir(&self, p: impl AsRef<Path>) -> Result<Self> {
        Self::new(self.join(p)?)
    }

    #[inline]
    pub fn create(&self, p: impl AsRef<Path>) -> Result<File> {
        Ok(File::create(self.join(p)?)?)
    }

    #[inline]
    pub fn open(&self, p: impl AsRef<Path>) -> Result<File> {
        Ok(File::open(self.join(p)?)?)
    }

    #[inline]
    pub fn exists(&self, p: impl AsRef<Path>) -> Result<bool> {
        Ok(self.join(p)?.exists())
    }

    #[inline]
    pub fn read(&self, p: impl AsRef<Path>) -> Result<Vec<u8>> {
        Ok(std::fs::read(self.join(p)?)?)
    }

    #[inline]
    pub fn read_dir(&self, p: impl AsRef<Path>) -> Result<ReadDir> {
        Ok(std::fs::read_dir(self.join(p)?)?)
    }
}
