// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! `kerosene-tools workshop`: put a map on the Steam Workshop.
//!
//! ```text
//! kerosene-tools workshop upload content/my_map.vault --title "My Map"
//! kerosene-tools workshop upload content/my_map.vault --item 3141592653 --note "Fixed the leak"
//! ```
//!
//! The other half of the thesis in `positioning.md`: a map is a `.vault`, a
//! subscribed Workshop item is a folder, and the engine mounts every archive
//! in every subscribed folder as one more layer of its file system. So
//! uploading is putting the archive in a folder and handing Steam the folder.
//! Nothing about the format changes for the Workshop.
//!
//! Needs the toolset built with `--features steam`, the Steam client running
//! and signed in to an account that owns the game, and the project's
//! `steam_appid`. With no `--item`, a new item is made and its id printed;
//! pass that id next time to update it rather than make another.

use anyhow::{Context, Result, bail};
use std::path::PathBuf;

const USAGE: &str = "usage: kerosene-tools workshop upload <file.vault | folder> \
    [--item <id>] [--title <text>] [--description <text>] [--preview <image>] \
    [--note <change note>] [--visibility public|friends|private|unlisted] [--tag <tag>]...";

/// What `workshop upload` was asked to do.
#[derive(Debug, Default, PartialEq)]
pub struct Upload {
    pub content: PathBuf,
    pub item: Option<u64>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub preview: Option<PathBuf>,
    pub note: Option<String>,
    pub visibility: Option<String>,
    pub tags: Vec<String>,
}

/// Read `upload`'s arguments.
pub fn parse(args: &[String]) -> Result<Upload> {
    let mut it = args.iter();
    match it.next().map(String::as_str) {
        Some("upload") => {}
        _ => bail!("{USAGE}"),
    }
    let mut upload = Upload::default();
    let mut content = None;
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| {
            it.next()
                .cloned()
                .with_context(|| format!("{flag} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "--item" => {
                let v = value("--item")?;
                upload.item = Some(
                    v.parse()
                        .with_context(|| format!("item `{v}` is not an id"))?,
                );
            }
            "--title" => upload.title = Some(value("--title")?),
            "--description" => upload.description = Some(value("--description")?),
            "--preview" => upload.preview = Some(PathBuf::from(value("--preview")?)),
            "--note" => upload.note = Some(value("--note")?),
            "--visibility" => {
                let v = value("--visibility")?;
                if !["public", "friends", "private", "unlisted"].contains(&v.as_str()) {
                    bail!("visibility `{v}`: public, friends, private or unlisted");
                }
                upload.visibility = Some(v);
            }
            "--tag" => upload.tags.push(value("--tag")?),
            flag if flag.starts_with("--") => bail!("unknown option {flag}\n{USAGE}"),
            path if content.is_none() => content = Some(PathBuf::from(path)),
            extra => bail!("unexpected `{extra}`\n{USAGE}"),
        }
    }
    upload.content = content.with_context(|| USAGE.to_string())?;
    if upload.item.is_none() && upload.title.is_none() {
        bail!("a new item needs a --title (or pass --item <id> to update one)");
    }
    Ok(upload)
}

/// The folder Steam uploads: the one given, or a folder made to hold a
/// single archive.
pub fn stage(content: &std::path::Path, staging_root: &std::path::Path) -> Result<PathBuf> {
    if content.is_dir() {
        return Ok(content.to_path_buf());
    }
    if !content.is_file() {
        bail!("{} does not exist", content.display());
    }
    let stem = content
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "item".into());
    let dir = staging_root.join(stem);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    std::fs::copy(content, dir.join(content.file_name().unwrap_or_default()))
        .with_context(|| format!("copying {}", content.display()))?;
    Ok(dir)
}

pub fn run(args: Vec<String>) -> Result<()> {
    let upload = parse(&args)?;
    let found = kerosene_vfs::root::find(None, None);
    let appid = found
        .as_ref()
        .and_then(|f| f.project.as_ref())
        .and_then(|p| p.steam_appid)
        .context("no steam_appid in the project; the Workshop belongs to an app")?;
    let staging = std::env::temp_dir().join("kerosene-workshop");
    let folder = stage(&upload.content, &staging)?;
    submit(appid, &upload, &folder)
}

#[cfg(not(feature = "steam"))]
fn submit(_: u32, _: &Upload, _: &std::path::Path) -> Result<()> {
    bail!(
        "this toolset was built without Steam. Rebuild it with \
         `cargo build -p kerosene-tools --features steam`."
    )
}

#[cfg(feature = "steam")]
fn submit(appid: u32, upload: &Upload, folder: &std::path::Path) -> Result<()> {
    use std::sync::mpsc::channel;
    use std::time::{Duration, Instant};
    use steamworks::{AppId, Client, FileType, PublishedFileId, PublishedFileVisibility};

    let client = Client::init_app(AppId(appid))
        .map_err(|e| anyhow::anyhow!("steam: {e}. Is the client running and signed in?"))?;
    let ugc = client.ugc();

    // Asynchronous calls answer through callbacks, which only run while
    // they are pumped.
    let wait = |rx: &std::sync::mpsc::Receiver<_>, what: &str| {
        let started = Instant::now();
        loop {
            client.run_callbacks();
            if let Ok(v) = rx.try_recv() {
                return Ok(v);
            }
            if started.elapsed() > Duration::from_secs(600) {
                bail!("steam did not answer ({what})");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    };

    let item = match upload.item {
        Some(id) => PublishedFileId(id),
        None => {
            let (tx, rx) = channel();
            ugc.create_item(AppId(appid), FileType::Community, move |r| {
                let _ = tx.send(r);
            });
            let (id, legal) = wait(&rx, "creating the item")?
                .map_err(|e| anyhow::anyhow!("steam refused to create the item: {e}"))?;
            println!("created workshop item {}", id.0);
            if legal {
                println!(
                    "  accept the Workshop legal agreement before it can be seen: \
                     https://steamcommunity.com/sharedfiles/workshoplegalagreement"
                );
            }
            id
        }
    };

    let mut update = ugc
        .start_item_update(AppId(appid), item)
        .content_path(folder);
    if let Some(t) = &upload.title {
        update = update.title(t);
    }
    if let Some(d) = &upload.description {
        update = update.description(d);
    }
    if let Some(p) = &upload.preview {
        let p = p
            .canonicalize()
            .with_context(|| format!("preview {}", p.display()))?;
        update = update.preview_path(&p);
    }
    if let Some(v) = &upload.visibility {
        update = update.visibility(match v.as_str() {
            "public" => PublishedFileVisibility::Public,
            "friends" => PublishedFileVisibility::FriendsOnly,
            "unlisted" => PublishedFileVisibility::Unlisted,
            _ => PublishedFileVisibility::Private,
        });
    }
    if !upload.tags.is_empty() {
        update = update.tags(upload.tags.clone(), false);
    }

    let (tx, rx) = channel();
    let watch = update.submit(upload.note.as_deref(), move |r| {
        let _ = tx.send(r);
    });
    println!("uploading {}", folder.display());
    let started = Instant::now();
    let result = loop {
        client.run_callbacks();
        if let Ok(r) = rx.try_recv() {
            break r;
        }
        let (status, done, total) = watch.progress();
        if total > 0 {
            print!("\r  {status:?}: {done} / {total} bytes   ");
            use std::io::Write;
            let _ = std::io::stdout().flush();
        }
        if started.elapsed() > Duration::from_secs(3600) {
            bail!("the upload did not finish within an hour");
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    println!();
    let (id, legal) = result.map_err(|e| anyhow::anyhow!("steam refused the update: {e}"))?;
    println!(
        "workshop item {} updated: https://steamcommunity.com/sharedfiles/filedetails/?id={}",
        id.0, id.0
    );
    if legal {
        println!("  the Workshop legal agreement still needs accepting");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn upload_arguments_are_read() {
        let u = parse(&args(&[
            "upload",
            "maps/a.vault",
            "--title",
            "A",
            "--tag",
            "Map",
            "--tag",
            "Co-op",
            "--visibility",
            "friends",
        ]))
        .unwrap();
        assert_eq!(u.content, PathBuf::from("maps/a.vault"));
        assert_eq!(u.title.as_deref(), Some("A"));
        assert_eq!(u.tags, vec!["Map", "Co-op"]);
        assert_eq!(u.visibility.as_deref(), Some("friends"));
    }

    #[test]
    fn a_new_item_needs_a_title_and_an_update_does_not() {
        assert!(parse(&args(&["upload", "a.vault"])).is_err());
        let u = parse(&args(&["upload", "a.vault", "--item", "42"])).unwrap();
        assert_eq!(u.item, Some(42));
        assert!(parse(&args(&["upload", "a.vault", "--item", "x"])).is_err());
        assert!(parse(&args(&["download"])).is_err());
        assert!(
            parse(&args(&[
                "upload",
                "a",
                "--visibility",
                "everyone",
                "--title",
                "t"
            ]))
            .is_err()
        );
    }

    #[test]
    fn a_single_archive_is_staged_in_a_folder_of_its_own() {
        let root = std::env::temp_dir().join(format!("kt-workshop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("my_map.vault");
        std::fs::write(&file, b"vault").unwrap();
        let folder = stage(&file, &root.join("staging")).unwrap();
        assert_eq!(folder, root.join("staging/my_map"));
        assert_eq!(
            std::fs::read(folder.join("my_map.vault")).unwrap(),
            b"vault"
        );
        assert_eq!(stage(&root, &root.join("s2")).unwrap(), root);
        assert!(stage(&root.join("missing.vault"), &root).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
