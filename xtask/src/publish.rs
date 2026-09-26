// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! `cargo xtask publish`: bundle Kerosene and publish it, from the
//! repository root.
//!
//! ```text
//! cargo xtask publish --dry-run   # everything but the upload
//! cargo xtask publish             # the real thing
//! cargo xtask publish --yes       # answer yes to every question (CI)
//! ```
//!
//! A version on crates.io is there for good -- it can be yanked, never
//! replaced -- so before anything is uploaded this looks for the usual
//! mistakes and says so:
//!
//! * **Stops** if this version is already on crates.io, or is older than one
//!   that is: crates.io would refuse it, or a game would never be offered it.
//! * **Asks** if anything is uncommitted or unpushed, if this version is
//!   already tagged at an older commit (the bump was forgotten), or if the
//!   changelog has no section for it (`scripts/bump-version.sh` was not run).
//!
//! Then it runs `cargo xtask bundle` and `cargo publish` in the bundle, and
//! offers to tag the release.

use anyhow::{Context, Result, bail};
use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;
use std::process::Command;

/// What `publish` was asked.
#[derive(Debug, Default, PartialEq)]
struct Args {
    dry_run: bool,
    yes: bool,
}

fn parse(args: &[String]) -> Result<Args> {
    let mut parsed = Args::default();
    for arg in args {
        match arg.as_str() {
            "--dry-run" | "-n" => parsed.dry_run = true,
            "--yes" | "-y" => parsed.yes = true,
            other => bail!("unknown argument {other:?}. Try --dry-run or --yes."),
        }
    }
    Ok(parsed)
}

/// Something found before publishing.
#[derive(Debug, PartialEq)]
enum Finding {
    /// Publishing would fail, or should not happen: stop.
    Stop(String),
    /// Probably a mistake, possibly not: ask.
    Ask(String),
    /// Worth saying, not worth asking about.
    Note(String),
}

pub fn run(args: &[String]) -> Result<()> {
    let args = parse(args)?;
    let repo = crate::repo_root()?;
    let version = workspace_version(&repo)?;
    println!("Kerosene {version}");

    let findings = check(&repo, &version);
    let mut stop = false;
    let mut ask = false;
    for finding in &findings {
        match finding {
            Finding::Stop(text) => {
                stop = true;
                println!("  error: {text}");
            }
            Finding::Ask(text) => {
                ask = true;
                println!("  warning: {text}");
            }
            Finding::Note(text) => println!("  note: {text}"),
        }
    }
    if stop && !args.dry_run {
        bail!("not publishing");
    }
    if stop {
        println!("  (a dry run uploads nothing, so it carries on to show the rest)");
    }
    if findings.is_empty() {
        println!("  committed, pushed, bumped and in the changelog");
    }
    if ask && !args.dry_run && !confirm("Publish anyway?", false, args.yes)? {
        bail!("not publishing");
    }

    println!("==> bundle");
    let out = repo.join("target/bundle/kerosene");
    println!("{}", crate::bundle::bundle(&repo, &out)?);

    println!(
        "==> cargo publish{}",
        if args.dry_run { " --dry-run" } else { "" }
    );
    let mut publish = Command::new(cargo());
    publish
        .arg("publish")
        // The bundle is generated into `target/`, which git ignores; the
        // checks above are what stand in for Cargo's own.
        .arg("--allow-dirty")
        .current_dir(&out)
        .env("CARGO_TARGET_DIR", repo.join("target/bundle/target"));
    if args.dry_run {
        publish.arg("--dry-run");
    }
    let status = publish.status().context("running cargo publish")?;
    if !status.success() {
        bail!("cargo publish failed ({status})");
    }
    if args.dry_run {
        println!("\ndry run: nothing was uploaded. `cargo xtask publish` to publish.");
        return Ok(());
    }

    println!("\npublished kerosene {version}");
    if !git_ok(
        &repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/tags/{version}"),
        ],
    ) {
        if confirm(&format!("Tag this commit as {version}?"), true, args.yes)? {
            if git_ok(&repo, &["tag", &version]) {
                println!("tagged {version}. Push it with: git push origin {version}");
                println!("(pushing the tag runs the release workflow)");
            } else {
                println!("could not tag; tag it yourself: git tag {version}");
            }
        } else {
            println!("not tagged. When you do: git tag {version} && git push origin {version}");
        }
    }
    Ok(())
}

/// Everything worth knowing before publishing `version`.
fn check(repo: &Path, version: &str) -> Vec<Finding> {
    let mut findings = Vec::new();

    // crates.io: already there, or older than what is.
    match published_versions() {
        Ok(published) => {
            if published.iter().any(|v| v == version) {
                findings.push(Finding::Stop(format!(
                    "{version} is already on crates.io, and a version can only be published \
                     once. Bump it: scripts/bump-version.sh <next>"
                )));
            } else if let Some(newest) = published.iter().max_by(|a, b| compare(a, b))
                && compare(version, newest).is_lt()
            {
                findings.push(Finding::Stop(format!(
                    "{version} is older than {newest}, which is already on crates.io"
                )));
            }
        }
        Err(e) => findings.push(Finding::Note(format!(
            "could not ask crates.io which versions it has ({e}); \
             cargo publish will refuse a repeat anyway"
        ))),
    }

    // Git: uncommitted, unpushed, and a tag that says the bump was missed.
    match git_output(repo, &["status", "--porcelain"]) {
        Some(status) => {
            let changed = uncommitted(&status);
            if !changed.is_empty() {
                let shown: Vec<&str> = changed.iter().take(8).map(String::as_str).collect();
                let more = changed.len().saturating_sub(shown.len());
                findings.push(Finding::Ask(format!(
                    "{} uncommitted change(s), which would be published but are in no \
                     commit: {}{}",
                    changed.len(),
                    shown.join(", "),
                    if more > 0 {
                        format!(" and {more} more")
                    } else {
                        String::new()
                    }
                )));
            }
        }
        None => findings.push(Finding::Note(
            "not a git checkout; nothing to compare".into(),
        )),
    }
    match git_output(repo, &["rev-list", "--count", "@{upstream}..HEAD"]) {
        Some(n) if n.trim() != "0" => findings.push(Finding::Ask(format!(
            "{} commit(s) not pushed: crates.io would have code the repository does not",
            n.trim()
        ))),
        Some(_) => {}
        None => findings.push(Finding::Ask(
            "this branch has no upstream, so what is published cannot be found in the \
             repository"
                .into(),
        )),
    }
    let tag = format!("refs/tags/{version}");
    if let (Some(tagged), Some(head)) = (
        git_output(repo, &["rev-list", "-n", "1", &tag]),
        git_output(repo, &["rev-parse", "HEAD"]),
    ) && tagged.trim() != head.trim()
    {
        findings.push(Finding::Ask(format!(
            "{version} is already tagged, at an older commit. Did you forget to bump the \
             version? scripts/bump-version.sh <next>"
        )));
    }

    // The changelog: bump-version.sh dates a section for the version.
    let changelog = std::fs::read_to_string(repo.join("CHANGELOG.md")).unwrap_or_default();
    if !has_section(&changelog, version) {
        findings.push(Finding::Ask(format!(
            "CHANGELOG.md has no section for {version}. scripts/bump-version.sh makes one \
             from Unreleased"
        )));
    }
    findings
}

/// The files `git status --porcelain` lists.
fn uncommitted(porcelain: &str) -> Vec<String> {
    porcelain
        .lines()
        .filter(|l| l.len() > 3)
        .map(|l| l[3..].trim().to_string())
        .collect()
}

/// Whether a changelog has a `## [version]` heading.
fn has_section(changelog: &str, version: &str) -> bool {
    changelog
        .lines()
        .any(|l| l.starts_with(&format!("## [{version}]")))
}

/// The workspace's version, from `[workspace.package]`.
fn workspace_version(repo: &Path) -> Result<String> {
    let manifest = std::fs::read_to_string(repo.join("Cargo.toml"))?;
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[workspace.package]";
        } else if in_package && let Some(v) = line.strip_prefix("version = ") {
            return Ok(v.trim_matches('"').to_string());
        }
    }
    bail!("no version in [workspace.package] of Cargo.toml")
}

/// The versions of `kerosene` crates.io has, yanked or not: a version can
/// be published only once either way. Read from the sparse index with
/// `curl`, which every platform Kerosene builds on has.
fn published_versions() -> Result<Vec<String>> {
    let output = Command::new("curl")
        .args([
            "-sSf",
            "--max-time",
            "20",
            "https://index.crates.io/ke/ro/kerosene",
        ])
        .output()
        .context("running curl")?;
    if !output.status.success() {
        // 404: never published.
        let err = String::from_utf8_lossy(&output.stderr);
        if err.contains("404") {
            return Ok(Vec::new());
        }
        bail!("{}", err.trim());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v["vers"].as_str().map(str::to_string))
        .collect())
}

/// SemVer precedence: the numbers, then a pre-release below its release,
/// then the pre-release's identifiers, numeric ones numerically.
fn compare(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let split = |v: &str| -> (Vec<u64>, Option<String>) {
        let v = v.split('+').next().unwrap_or(v);
        let (core, pre) = match v.split_once('-') {
            Some((core, pre)) => (core, Some(pre.to_string())),
            None => (v, None),
        };
        (
            core.split('.').map(|n| n.parse().unwrap_or(0)).collect(),
            pre,
        )
    };
    let (core_a, pre_a) = split(a);
    let (core_b, pre_b) = split(b);
    match core_a.cmp(&core_b) {
        Ordering::Equal => {}
        other => return other,
    }
    match (pre_a, pre_b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => {
            let xs: Vec<&str> = x.split('.').collect();
            let ys: Vec<&str> = y.split('.').collect();
            for (p, q) in xs.iter().zip(&ys) {
                let o = match (p.parse::<u64>(), q.parse::<u64>()) {
                    (Ok(m), Ok(n)) => m.cmp(&n),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    // `a2` against `a10`: alphanumeric, but people write
                    // them expecting the number to count.
                    (Err(_), Err(_)) => natural(p, q),
                };
                if o != Ordering::Equal {
                    return o;
                }
            }
            xs.len().cmp(&ys.len())
        }
    }
}

/// `a2` before `a10`: letters compared as letters, digits as a number.
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let split = |s: &str| {
        let digits = s.trim_start_matches(|c: char| !c.is_ascii_digit());
        let prefix = &s[..s.len() - digits.len()];
        (prefix.to_string(), digits.parse::<u64>().ok())
    };
    let (pa, na) = split(a);
    let (pb, nb) = split(b);
    pa.cmp(&pb).then(na.cmp(&nb)).then(a.cmp(b))
}

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

fn git_output(repo: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn git_ok(repo: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .is_ok_and(|s| s.success())
}

/// Ask a yes/no question. `--yes` answers yes; with no terminal to ask on,
/// and no `--yes`, the answer is no.
fn confirm(question: &str, default: bool, yes: bool) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        println!("{question} (no terminal to ask on; pass --yes to answer yes)");
        return Ok(false);
    }
    print!("{question} [{}] ", if default { "Y/n" } else { "y/N" });
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(match line.trim().to_ascii_lowercase().as_str() {
        "" => default,
        answer => answer == "y" || answer == "yes",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering::*;

    #[test]
    fn versions_are_ordered_the_way_semver_orders_them() {
        assert_eq!(compare("1.0.0-a2", "1.0.0-a1"), Greater);
        assert_eq!(compare("1.0.0-a10", "1.0.0-a2"), Greater);
        assert_eq!(compare("1.0.0-b1", "1.0.0-a9"), Greater);
        assert_eq!(compare("1.0.0-rc.1", "1.0.0-b3"), Greater);
        assert_eq!(compare("1.0.0", "1.0.0-rc.2"), Greater);
        assert_eq!(compare("1.2.0", "1.10.0"), Less);
        assert_eq!(compare("1.0.0-a2", "1.0.0-a2"), Equal);
    }

    #[test]
    fn the_changelog_and_the_status_are_read_as_bump_version_and_git_write_them() {
        let changelog = "# Changelog\n\n## [Unreleased]\n\n## [1.0.0-a2] - 2026-09-26\n";
        assert!(has_section(changelog, "1.0.0-a2"));
        assert!(!has_section(changelog, "1.0.0-a3"));
        assert_eq!(
            uncommitted(" M Cargo.toml\n?? xtask/src/new.rs\n"),
            ["Cargo.toml", "xtask/src/new.rs"]
        );
        assert!(uncommitted("").is_empty());
    }

    #[test]
    fn publish_takes_dry_run_and_yes_and_nothing_else() {
        let a = |v: &[&str]| parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(
            a(&["--dry-run", "-y"]).unwrap(),
            Args {
                dry_run: true,
                yes: true
            }
        );
        assert!(a(&["--force"]).is_err());
    }
}
