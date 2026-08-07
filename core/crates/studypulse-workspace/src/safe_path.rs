//! Path validation shared by library, Agent, media, and backup operations.
//!
//! Wire paths are intentionally narrower than native paths: they use `/`, do
//! not contain parent traversal or drive prefixes, and are resolved only below
//! a canonical Workspace root.  The checks are repeated at the filesystem
//! boundary because a validated string alone does not prevent a symlink from
//! changing the meaning of a later path component.
use std::path::{Component, Path, PathBuf};

use crate::{Result, WorkspaceError, platform::is_link_like};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeRelativePath(PathBuf);

impl SafeRelativePath {
    /// Parse a wire path once and retain its normalized native representation.
    /// `.` denotes the Workspace root and is stored as an empty relative path
    /// so joining it does not introduce a special component later.
    pub fn parse(value: &str) -> Result<Self> {
        validate_wire_relative_path(value)?;
        let normalized = if value == "." { "" } else { value };
        Ok(Self(PathBuf::from(normalized)))
    }

    /// Borrow the validated path for joining or canonicalization.
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

pub fn validate_wire_relative_path(value: &str) -> Result<()> {
    // Reject alternate separators before `Path::components`, whose behavior is
    // platform-dependent and would otherwise make wire validation asymmetric.
    if value.contains('\\') {
        return Err(WorkspaceError::InvalidPath(value.into()));
    }
    // Absolute paths and UNC-like forms must never be interpreted relative to
    // the process' current directory or a different filesystem root.
    if value.starts_with('/') || value.starts_with("//") {
        return Err(WorkspaceError::InvalidPath(value.into()));
    }
    // A colon in the first segment covers Windows drive prefixes while still
    // allowing ordinary colons in later filename segments if a platform does.
    if value
        .split('/')
        .next()
        .is_some_and(|first| first.contains(':'))
    {
        return Err(WorkspaceError::InvalidPath(value.into()));
    }
    // Component validation catches `..`, roots, and platform prefixes after the
    // portable string checks above; normal and current-directory components are
    // the only forms that can remain inside the Workspace.
    let path = Path::new(value);
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(WorkspaceError::InvalidPath(value.into()));
            }
        }
    }
    // Empty segments would make `Documents//file` ambiguous across clients;
    // the empty string itself remains valid for the Workspace root.
    if value.split('/').any(|part| part == ".." || part.is_empty()) && !value.is_empty() {
        return Err(WorkspaceError::InvalidPath(value.into()));
    }
    Ok(())
}

pub(crate) fn ensure_no_symlink_components(root: &Path, relative: &Path) -> Result<()> {
    // Canonicalize the root once, then inspect every existing component without
    // following links implicitly.  This protects both existing files and
    // parent directories created by callers after validation.
    let canonical_root = root.canonicalize()?;
    let mut current = canonical_root.clone();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            // SafeRelativePath normally removes these; ignoring non-normal
            // components here keeps this helper defensive if called internally.
            continue;
        };
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if is_link_like(&metadata) => {
                // A link is rejected even when its target happens to be inside
                // the root: forbidding links keeps later mutations stable.
                return Err(WorkspaceError::SymbolicLink(
                    relative.to_string_lossy().into_owned(),
                ));
            }
            Ok(_) => {
                let resolved = current.canonicalize()?;
                if !resolved.starts_with(&canonical_root) {
                    // Canonical containment is the second guard against a
                    // junction/reparse point escaping the root.
                    return Err(WorkspaceError::PathEscape(
                        relative.to_string_lossy().into_owned(),
                    ));
                }
            }
            // A missing tail is safe to inspect later; callers that create it
            // still use an explicitly constructed relative path.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // Portable rejection cases cover traversal, absolute, drive, UNC, and
    // duplicate-separator spellings before platform-specific path handling.
    fn rejects_portable_escape_forms() {
        for value in [
            "../secret",
            "Documents/../secret",
            "/tmp/file",
            r"C:/Users/file",
            r"\\server/share",
            "Documents\\file",
            "Documents//file",
        ] {
            assert!(validate_wire_relative_path(value).is_err(), "{value}");
        }
    }

    #[test]
    // Empty/root paths and nested normal components remain valid inputs.
    fn accepts_normal_workspace_paths() {
        for value in ["", ".", "Documents", "Notes/math/algebra.md"] {
            assert!(validate_wire_relative_path(value).is_ok(), "{value}");
        }
    }
}
