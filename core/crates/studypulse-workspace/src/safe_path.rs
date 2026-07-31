use std::path::{Component, Path, PathBuf};

use crate::{Result, WorkspaceError, platform::is_link_like};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeRelativePath(PathBuf);

impl SafeRelativePath {
    pub fn parse(value: &str) -> Result<Self> {
        validate_wire_relative_path(value)?;
        let normalized = if value == "." { "" } else { value };
        Ok(Self(PathBuf::from(normalized)))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

pub fn validate_wire_relative_path(value: &str) -> Result<()> {
    if value.contains('\\') {
        return Err(WorkspaceError::InvalidPath(value.into()));
    }
    if value.starts_with('/') || value.starts_with("//") {
        return Err(WorkspaceError::InvalidPath(value.into()));
    }
    if value
        .split('/')
        .next()
        .is_some_and(|first| first.contains(':'))
    {
        return Err(WorkspaceError::InvalidPath(value.into()));
    }
    let path = Path::new(value);
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(WorkspaceError::InvalidPath(value.into()));
            }
        }
    }
    if value.split('/').any(|part| part == ".." || part.is_empty()) && !value.is_empty() {
        return Err(WorkspaceError::InvalidPath(value.into()));
    }
    Ok(())
}

pub(crate) fn ensure_no_symlink_components(root: &Path, relative: &Path) -> Result<()> {
    let canonical_root = root.canonicalize()?;
    let mut current = canonical_root.clone();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            continue;
        };
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if is_link_like(&metadata) => {
                return Err(WorkspaceError::SymbolicLink(
                    relative.to_string_lossy().into_owned(),
                ));
            }
            Ok(_) => {
                let resolved = current.canonicalize()?;
                if !resolved.starts_with(&canonical_root) {
                    return Err(WorkspaceError::PathEscape(
                        relative.to_string_lossy().into_owned(),
                    ));
                }
            }
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
    fn accepts_normal_workspace_paths() {
        for value in ["", ".", "Documents", "Notes/math/algebra.md"] {
            assert!(validate_wire_relative_path(value).is_ok(), "{value}");
        }
    }
}
