//! Virtual path normalisation.
//!
//! Asset paths come from map files, material references and tool arguments,
//! authored on whatever platform, so they arrive in every spelling:
//! `Materials\Dev\Grid`, `materials/dev/grid`, `./materials//dev/grid`. They
//! all have to resolve to one key, and none of them may escape the game tree.

/// Normalise a virtual path: lowercase, forward slashes, no redundant parts.
///
/// Returns `None` if the path tries to climb above the search root. That check
/// is not a nicety: map and material files are untrusted content that ships
/// between users, and `../../../../etc/passwd` in a `$basetexture` must not
/// resolve to anything.
pub fn normalize(path: &str) -> Option<String> {
    let mut out: Vec<&str> = Vec::new();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => continue,
            ".." => {
                // Refuse rather than clamp: silently resolving to the root
                // would let a crafted path reach a sibling it should not see.
                out.pop()?;
            }
            p => out.push(p),
        }
    }
    if out.is_empty() { return None; }
    Some(out.join("/").to_lowercase())
}

/// Lowercase extension without the dot, if any.
pub fn extension(path: &str) -> Option<String> {
    let file = path.rsplit('/').next()?;
    let (_, ext) = file.rsplit_once('.')?;
    (!ext.is_empty()).then(|| ext.to_lowercase())
}

/// Directory portion of a virtual path, or `""` at the root.
pub fn parent(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

/// Replace or add an extension.
pub fn with_extension(path: &str, ext: &str) -> String {
    let base = match path.rfind('.') {
        Some(i) if !path[i..].contains('/') => &path[..i],
        _ => path,
    };
    format!("{base}.{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings_collapse_to_one_key() {
        let want = Some("materials/dev/grid.vmat".to_string());
        for src in [
            r"Materials\Dev\Grid.vmat",
            "materials/dev/grid.vmat",
            "./materials//dev/grid.vmat",
            "/materials/dev/grid.vmat",
        ] {
            assert_eq!(normalize(src), want, "{src}");
        }
    }

    #[test]
    fn interior_dotdot_resolves() {
        assert_eq!(normalize("materials/dev/../props/x.vmat").as_deref(), Some("materials/props/x.vmat"));
    }

    #[test]
    fn escaping_the_root_is_refused() {
        // The security case: a hostile material reference must not reach out.
        for bad in ["../secrets", "materials/../../etc/passwd", "..", "a/../.."] {
            assert_eq!(normalize(bad), None, "{bad} should be refused");
        }
    }

    #[test]
    fn empty_paths_have_no_key() {
        assert_eq!(normalize(""), None);
        assert_eq!(normalize("///"), None);
    }

    #[test]
    fn extension_and_parent() {
        assert_eq!(extension("a/b/c.VMAT").as_deref(), Some("vmat"));
        assert_eq!(extension("a/b/noext"), None);
        assert_eq!(parent("a/b/c.vmat"), "a/b");
        assert_eq!(parent("c.vmat"), "");
        assert_eq!(with_extension("maps/void_start.vmap", "vbsp"), "maps/void_start.vbsp");
        assert_eq!(with_extension("maps/noext", "vbsp"), "maps/noext.vbsp");
    }
}
