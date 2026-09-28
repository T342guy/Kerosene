// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Compiled resources: the one container they are written in, the handles
//! the runtime holds them by, and the table of asset types.
//!
//! Every asset exists twice: a source a person edits (`.png`, `.kmat`,
//! `.kmap`) and a compiled file the runtime loads. The runtime never reads
//! the first kind -- that rule is what lets the tools change a source
//! format, check a compiled file for staleness, and package a game without
//! special cases. This crate is the runtime's end of it.
//!
//! - [`container`]: the common layout. A header naming the kind and the
//!   hash of the source it came from, then typed blocks: the payload, the
//!   other resources it needs, and how it was compiled.
//! - [`ResourceType`]: what a type implements to be read out of one, and
//!   [`decode`], which checks the kind and version first.
//! - [`Resources`] and [`Resource<T>`]: a cache of handles, loaded now or
//!   later, reloaded in place.
//! - [`types`]: which source compiles to which file, and by what.
//!
//! Formats move into the container one at a time. A type whose older
//! files are still about keeps reading them through
//! [`ResourceType::decode_legacy`] until they are rebuilt.

pub mod bytes;
pub mod container;
mod handle;
pub mod types;

pub use container::{
    CONTAINER_VERSION, EditInfo, MAGIC, ResourceFile, ResourceView, is_resource, source_hash, tag,
};
pub use handle::{LoadState, Resource, Resources, Source};
pub use types::AssetType;

use thiserror::Error;

/// Why a resource would not read.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ResourceError {
    #[error("not a compiled resource (no KRES header)")]
    NotAResource,
    #[error("truncated: needs {needed} bytes, has {available}")]
    Truncated { needed: usize, available: usize },
    #[error("container version {0}; this build reads version {CONTAINER_VERSION}")]
    ContainerVersion(u32),
    #[error("a {found} resource, where a {expected} was wanted")]
    WrongKind { expected: String, found: String },
    #[error("{kind} version {found}; this build reads up to version {supported}")]
    TooNew {
        kind: String,
        found: u32,
        supported: u32,
    },
    #[error("no {0} block")]
    MissingBlock(String),
    #[error("{tag} block: {reason}")]
    BadBlock { tag: String, reason: String },
    /// The payload is damaged or says something impossible.
    #[error("{0}")]
    Invalid(String),
    /// The file would not read.
    #[error("{0}")]
    Read(String),
}

/// A type that can be read out of a compiled resource.
pub trait ResourceType: Sized + Send + Sync + 'static {
    /// The four bytes naming it in the container header.
    const KIND: [u8; 4];
    /// The newest payload layout this build reads and writes.
    const VERSION: u32;

    /// Read the payload. The kind has been checked, and the version is at
    /// most [`ResourceType::VERSION`]; an older one is this function's to
    /// handle or refuse.
    fn decode(file: &ResourceView<'_>) -> Result<Self, ResourceError>;

    /// Read a file from before the type moved into the container. `None`
    /// when `bytes` is not one of those either.
    fn decode_legacy(_bytes: &[u8]) -> Option<Result<Self, ResourceError>> {
        None
    }
}

/// Read a `T` from a compiled file's bytes.
pub fn decode<T: ResourceType>(bytes: &[u8]) -> Result<T, ResourceError> {
    if !is_resource(bytes) {
        return T::decode_legacy(bytes).unwrap_or(Err(ResourceError::NotAResource));
    }
    let file = ResourceView::parse(bytes)?;
    if file.kind() != T::KIND {
        return Err(ResourceError::WrongKind {
            expected: fourcc(T::KIND),
            found: fourcc(file.kind()),
        });
    }
    if file.kind_version() > T::VERSION {
        return Err(ResourceError::TooNew {
            kind: fourcc(T::KIND),
            found: file.kind_version(),
            supported: T::VERSION,
        });
    }
    T::decode(&file)
}

/// Four bytes as text, for messages.
pub fn fourcc(code: [u8; 4]) -> String {
    code.iter()
        .map(|&b| if b.is_ascii_graphic() { b as char } else { '?' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Legacy(u8);

    impl ResourceType for Legacy {
        const KIND: [u8; 4] = *b"LGCY";
        const VERSION: u32 = 2;
        fn decode(file: &ResourceView<'_>) -> Result<Self, ResourceError> {
            Ok(Legacy(file.require(tag::DATA)?[0]))
        }
        fn decode_legacy(bytes: &[u8]) -> Option<Result<Self, ResourceError>> {
            bytes
                .strip_prefix(b"OLD!")
                .map(|rest| Ok(Legacy(rest.first().copied().unwrap_or(0))))
        }
    }

    #[test]
    fn an_old_file_still_reads_through_the_legacy_reader() {
        assert_eq!(decode::<Legacy>(b"OLD!\x05").unwrap().0, 5);
        assert!(matches!(
            decode::<Legacy>(b"what is this"),
            Err(ResourceError::NotAResource)
        ));
    }

    #[test]
    fn a_newer_payload_is_refused_and_an_older_one_is_not() {
        for (version, ok) in [(1, true), (2, true), (3, false)] {
            let mut f = ResourceFile::new(Legacy::KIND, version);
            f.push(tag::DATA, vec![9]);
            let got = decode::<Legacy>(&f.to_bytes());
            assert_eq!(got.is_ok(), ok, "version {version}");
        }
    }

    #[test]
    fn fourcc_never_prints_control_bytes() {
        assert_eq!(fourcc(*b"KMAT"), "KMAT");
        assert_eq!(fourcc([0, b'A', 0xff, b' ']), "?A??");
    }
}
