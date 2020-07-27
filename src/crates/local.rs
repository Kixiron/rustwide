use super::CrateTrait;
use crate::Workspace;
use async_trait::async_trait;
use failure::Error;
use log::info;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub(super) struct Local {
    path: PathBuf,
}

impl Local {
    pub(super) fn new(path: &Path) -> Self {
        Local { path: path.into() }
    }
}

#[async_trait]
impl CrateTrait for Local {
    async fn fetch(&self, _workspace: &Workspace) -> Result<(), Error> {
        // There is no fetch to do for a local crate.
        Ok(())
    }

    async fn purge_from_cache(&self, _workspace: &Workspace) -> Result<(), Error> {
        // There is no cache to purge for a local crate.
        Ok(())
    }

    async fn copy_source_to(&self, _workspace: &Workspace, dest: &Path) -> Result<(), Error> {
        info!(
            "copying local crate from {} to {}",
            self.path.display(),
            dest.display()
        );
        copy_dir(&self.path, dest).await?;

        Ok(())
    }
}

impl std::fmt::Display for Local {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "local crate {}", self.path.display())
    }
}

async fn copy_dir(src: &Path, dest: &Path) -> Result<(), Error> {
    let src = crate::utils::normalize_path(src);
    let dest = crate::utils::normalize_path(dest);

    let src_components = src.components().count();
    let mut entries = WalkDir::new(&src).follow_links(true).into_iter();
    while let Some(entry) = entries.next() {
        let entry = entry?;

        let mut components = entry.path().components();
        for _ in 0..src_components {
            components.next();
        }
        let path = components.as_path();

        if entry.file_type().is_dir() {
            // don't copy /target directory
            if entry.file_name() == "target" && entry.depth() == 1 {
                info!("ignoring top-level target directory {}", path.display());
                entries.skip_current_dir();
            } else {
                std::fs::create_dir_all(dest.join(path))?;
            }
        } else {
            std::fs::copy(src.join(path), dest.join(path))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use failure::Error;
    use tokio::fs;

    #[tokio::test]
    async fn test_copy_dir() -> Result<(), Error> {
        let tmp_src = tempfile::tempdir()?;
        let tmp_dest = tempfile::tempdir()?;

        // Create some files in the src dir
        fs::create_dir(tmp_src.path().join("dir")).await?;
        fs::write(tmp_src.path().join("foo"), b"Hello world").await?;
        fs::write(tmp_src.path().join("dir").join("bar"), b"Rustwide").await?;

        super::copy_dir(tmp_src.path(), tmp_dest.path()).await?;

        assert_eq!(fs::read(tmp_dest.path().join("foo")).await?, b"Hello world");
        assert_eq!(
            fs::read(tmp_dest.path().join("dir").join("bar")).await?,
            b"Rustwide"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_no_copy_target() -> Result<(), Error> {
        let (src, dest) = (tempfile::tempdir()?, tempfile::tempdir()?);
        fs::create_dir(src.path().join("target")).await?;
        fs::write(
            src.path().join("target").join("a.out"),
            b"this is not actually an ELF file",
        )
        .await?;
        println!("made subdirs and files");

        super::copy_dir(src.path(), dest.path()).await?;
        println!("copied");

        assert!(!dest.path().join("target").exists());
        Ok(())
    }

    #[tokio::test]
    async fn test_copy_symlinks() -> Result<(), Error> {
        use std::{os, path::Path};

        let tmp_src = tempfile::tempdir()?;
        let tmp_dest = tempfile::tempdir()?;
        let assert_copy_err_has_filename = async {
            match super::copy_dir(tmp_src.path(), tmp_dest.path()).await {
                Ok(_) => panic!("copy with bad symbolic link did not fail"),
                Err(err) => assert!(err.downcast::<walkdir::Error>().unwrap().path().is_some()),
            };
        };

        // Create some files in the src dir
        fs::create_dir(tmp_src.path().join("dir")).await?;
        fs::write(tmp_src.path().join("foo"), b"Hello world").await?;
        fs::write(tmp_src.path().join("dir").join("bar"), b"Rustwide").await?;
        let bad_link = tmp_src.path().join("symlink");

        // test link to non-existent file
        #[cfg(unix)]
        os::unix::fs::symlink(Path::new("/does_not_exist"), &bad_link)?;
        #[cfg(windows)]
        os::windows::fs::symlink_file(Path::new(r"C:\does_not_exist"), &bad_link)?;
        #[cfg(not(any(unix, windows)))]
        panic!("testing symbolic links not supported except on windows and linux");

        println!("{} should cause copy to fail", bad_link.display());
        assert_copy_err_has_filename.await;

        fs::remove_file(&bad_link).await?;
        // make sure it works without that link
        super::copy_dir(tmp_src.path(), tmp_dest.path()).await?;

        // test link to self
        #[cfg(unix)]
        os::unix::fs::symlink(&bad_link, &bad_link)?;
        #[cfg(windows)]
        os::windows::fs::symlink_file(&bad_link, &bad_link)?;

        println!("{} should cause copy to fail", bad_link.display());
        assert_copy_err_has_filename.await;
        Ok(())
    }
}
