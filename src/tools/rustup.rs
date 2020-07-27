use crate::cmd::{Binary, Command, Runnable};
use crate::toolchain::MAIN_TOOLCHAIN_NAME;
use crate::tools::{Tool, RUSTUP};
use crate::workspace::Workspace;
use async_trait::async_trait;
use failure::{Error, ResultExt};
use std::env::consts::EXE_SUFFIX;
use tempfile::tempdir;
use tokio::{
    fs::{self, File},
    io,
};

static RUSTUP_BASE_URL: &str = "https://static.rust-lang.org/rustup/dist";

pub(crate) struct Rustup;

impl Runnable for Rustup {
    fn name(&self) -> Binary {
        Binary::ManagedByRustwide("rustup".into())
    }
}

#[async_trait]
impl Tool for Rustup {
    fn name(&self) -> &'static str {
        "rustup"
    }

    fn is_installed(&self, workspace: &Workspace) -> Result<bool, Error> {
        let path = self.binary_path(workspace);
        if !path.is_file() {
            return Ok(false);
        }

        Ok(crate::native::is_executable(path)?)
    }

    async fn install(&self, workspace: &Workspace, _fast_install: bool) -> Result<(), Error> {
        fs::create_dir_all(workspace.cargo_home()).await?;
        fs::create_dir_all(workspace.rustup_home()).await?;

        let url = format!(
            "{}/{}/rustup-init{}",
            RUSTUP_BASE_URL,
            crate::HOST_TARGET,
            EXE_SUFFIX
        );
        let mut resp = workspace
            .http_client()
            .get(&url)
            .send()
            .await?
            .error_for_status()?;

        let tempdir = tempdir()?;
        let installer = &tempdir.path().join(format!("rustup-init{}", EXE_SUFFIX));
        {
            let mut file = File::create(installer).await?;
            io::copy(&mut resp, &mut file).await?;
            crate::native::make_executable(installer)?;
        }

        Command::new(workspace, installer.to_string_lossy().as_ref())
            .args(&[
                "-y",
                "--no-modify-path",
                "--default-toolchain",
                MAIN_TOOLCHAIN_NAME,
                "--profile",
                workspace.rustup_profile(),
            ])
            .env("RUSTUP_HOME", workspace.rustup_home())
            .env("CARGO_HOME", workspace.cargo_home())
            .run()
            .await
            .with_context(|_| "unable to install rustup")?;

        Ok(())
    }

    async fn update(&self, workspace: &Workspace, _fast_install: bool) -> Result<(), Error> {
        Command::new(workspace, &RUSTUP)
            .args(&["self", "update"])
            .run()
            .await
            .with_context(|_| "failed to update rustup")?;

        Command::new(workspace, &RUSTUP)
            .args(&["update", MAIN_TOOLCHAIN_NAME])
            .run()
            .await
            .with_context(|_| format!("failed to update main toolchain {}", MAIN_TOOLCHAIN_NAME))?;

        Ok(())
    }
}
