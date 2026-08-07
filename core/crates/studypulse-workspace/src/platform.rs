//! Small platform-specific filesystem predicates used by path traversal.
//!
//! Windows reparse points are treated like symbolic links because they can
//! redirect access outside the Workspace even when `file_type().is_symlink()`
//! is false.  Unix only needs the native symlink bit.
use std::fs::Metadata;

#[cfg(windows)]
pub(crate) fn is_link_like(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    // Junctions and other reparse points can redirect a path just like a link.
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
pub(crate) fn is_link_like(metadata: &Metadata) -> bool {
    // On Unix, symlink metadata is enough because the caller deliberately uses
    // `symlink_metadata` rather than following the entry first.
    // The predicate stays platform-local so the security rule is shared by all
    // higher-level Workspace operations.
    metadata.file_type().is_symlink()
}
