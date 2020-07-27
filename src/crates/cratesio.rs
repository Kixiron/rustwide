use super::CrateTrait;
use crate::Workspace;
use async_trait::async_trait;
use failure::Error;
use flate2::read::GzDecoder;
use log::info;
use remove_dir_all::remove_dir_all;
use std::io::Read;
use std::path::{Path, PathBuf};
use tar::Archive;
use tokio::{
    fs::{self, File},
    io::{BufReader, BufWriter},
};

static CRATES_ROOT: &str = "https://static.crates.io/crates";

impl CratesIOCrate {
    pub(super) fn new(name: &str, version: &str) -> Self {
        CratesIOCrate {
            name: name.into(),
            version: version.into(),
        }
    }

    fn cache_path(&self, workspace: &Workspace) -> PathBuf {
        workspace
            .cache_dir()
            .join("cratesio-sources")
            .join(&self.name)
            .join(format!("{}-{}.crate", self.name, self.version))
    }
}

pub(super) struct CratesIOCrate {
    name: String,
    version: String,
}

#[async_trait]
impl CrateTrait for CratesIOCrate {
    async fn fetch(&self, workspace: &Workspace) -> Result<(), Error> {
        let local = self.cache_path(workspace);
        if local.exists() {
            info!("crate {} {} is already in cache", self.name, self.version);
            return Ok(());
        }

        info!("fetching crate {} {}...", self.name, self.version);
        if let Some(parent) = local.parent() {
            fs::create_dir_all(parent).await?;
        }
        let remote = format!(
            "{0}/{1}/{1}-{2}.crate",
            CRATES_ROOT, self.name, self.version
        );
        let mut resp = workspace
            .http_client()
            .get(&remote)
            .send()
            .await?
            .error_for_status()?;
        resp.copy_to(&mut BufWriter::new(File::create(&local).await?))
            .await?;

        Ok(())
    }

    async fn purge_from_cache(&self, workspace: &Workspace) -> Result<(), Error> {
        let path = self.cache_path(workspace);
        if path.exists() {
            fs::remove_file(&path).await?;
        }

        Ok(())
    }

    async fn copy_source_to(&self, workspace: &Workspace, dest: &Path) -> Result<(), Error> {
        let cached = self.cache_path(workspace);
        let mut file = File::open(cached).await?;
        let mut tar = Archive::new(GzDecoder::new(BufReader::new(&mut file)));

        info!(
            "extracting crate {} {} into {}",
            self.name,
            self.version,
            dest.display()
        );
        if let Err(err) = unpack_without_first_dir(&mut tar, dest) {
            let _ = remove_dir_all(dest);
            Err(err
                .context(format!(
                    "unable to download {} version {}",
                    self.name, self.version
                ))
                .into())
        } else {
            Ok(())
        }
    }
}

impl std::fmt::Display for CratesIOCrate {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "crates.io crate {} {}", self.name, self.version)
    }
}

fn unpack_without_first_dir<R: Read>(archive: &mut Archive<R>, path: &Path) -> Result<(), Error> {
    let entries = archive.entries()?;
    for entry in entries {
        let mut entry = entry?;
        let relpath = {
            let path = entry.path();
            let path = path?;
            path.into_owned()
        };
        let mut components = relpath.components();
        // Throw away the first path component
        components.next();
        let full_path = path.join(&components.as_path());
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        entry.unpack(&full_path)?;
    }

    Ok(())
}
