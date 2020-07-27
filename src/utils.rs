use failure::Error;
use fs2::FileExt;
use futures_util::future::FutureExt;
use log::warn;
use std::{
    fs::OpenOptions,
    future::Future,
    panic,
    path::{Component, Path, PathBuf, Prefix, PrefixComponent},
};
use tokio::task;

pub(crate) async fn file_lock<T>(
    path: &Path,
    msg: &str,
    f: impl Future<Output = Result<T, Error>> + panic::UnwindSafe + Send + 'static,
) -> Result<T, Error> {
    let (path, msg) = (path.to_owned(), msg.to_owned());
    let file = task::spawn_blocking(move || {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        let mut message_displayed = false;
        while let Err(err) = file.try_lock_exclusive() {
            if !message_displayed && err.kind() == fs2::lock_contended_error().kind() {
                warn!("blocking on other processes finishing to {}", msg);
                message_displayed = true;
            }

            file.lock_exclusive()?;
        }

        Result::<_, Error>::Ok(file)
    })
    .await??;

    let res = f.catch_unwind().await;
    let _ = task::spawn_blocking(move || file.unlock()).await?;

    match res {
        Ok(res) => res,
        Err(panic) => panic::resume_unwind(panic),
    }
}

/// If a prefix uses the extended-length syntax (`\\?\`), return the equivalent version without it.
///
/// Returns `None` if `prefix.kind().is_verbatim()` is `false`.
fn strip_verbatim_from_prefix(prefix: &PrefixComponent<'_>) -> Option<PathBuf> {
    let ret = match prefix.kind() {
        Prefix::Verbatim(s) => Path::new(s).to_owned(),

        Prefix::VerbatimDisk(drive) => [format!(r"{}:\", drive as char)].iter().collect(),

        Prefix::VerbatimUNC(_, _) => unimplemented!(),

        _ => return None,
    };

    Some(ret)
}

pub(crate) fn normalize_path(path: &Path) -> PathBuf {
    let mut p = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    // `fs::canonicalize` returns an extended-length path on Windows. Such paths not supported by
    // many programs, including rustup. We strip the `\\?\` prefix of the canonicalized path, but
    // this changes the meaning of some path components, and imposes a length of around 260
    // characters.
    if cfg!(windows) {
        // A conservative estimate for the maximum length of a path on Windows.
        //
        // The additional 12 byte restriction is applied when creating directories. It ensures that
        // files can always be created inside that directory without exceeding the path limit.
        const MAX_PATH_LEN: usize = 260 - 12;

        let mut components = p.components();
        let first_component = components.next().unwrap();

        if let Component::Prefix(prefix) = first_component {
            if let Some(mut modified_path) = strip_verbatim_from_prefix(&prefix) {
                modified_path.push(components.as_path());
                p = modified_path;
            }
        }

        if p.as_os_str().len() >= MAX_PATH_LEN {
            warn!(
                "Canonicalized path is too long for Windows: {:?}",
                p.as_os_str(),
            );
        }
    }

    p
}

#[cfg(test)]
#[cfg(windows)]
mod windows_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn strip_verbatim() {
        let suite = vec![
            (r"C:\Users\carl", None),
            (r"\Users\carl", None),
            (r"\\?\C:\Users\carl", Some(r"C:\")),
            (r"\\?\Users\carl", Some(r"Users")),
        ];

        for (input, output) in suite {
            let p = Path::new(input);
            let first_component = p.components().next().unwrap();

            if let Component::Prefix(prefix) = &first_component {
                let stripped = strip_verbatim_from_prefix(&prefix);
                assert_eq!(stripped.as_ref().map(|p| p.to_str().unwrap()), output);
            } else {
                assert!(output.is_none());
            }
        }
    }
}
