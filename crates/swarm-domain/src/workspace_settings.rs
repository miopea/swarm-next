//! Durable discovery configuration, independent of filesystem and transport.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSearchSettings {
    pub revision: u64,
    pub folders: Vec<String>,
}

impl WorkspaceSearchSettings {
    /// Validates bounded, already-resolved folder identities.
    #[must_use]
    pub fn valid_folders(folders: &[String]) -> bool {
        folders.len() <= 16
            && folders
                .iter()
                .all(|folder| !folder.is_empty() && folder.len() <= 4096 && !folder.contains('\0'))
            && folders
                .iter()
                .enumerate()
                .all(|(index, folder)| !folders[..index].contains(folder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_and_unique_identities() {
        assert!(WorkspaceSearchSettings::valid_folders(&[]));
        assert!(WorkspaceSearchSettings::valid_folders(
            &["/projects".into()]
        ));
        assert!(!WorkspaceSearchSettings::valid_folders(&[
            "/projects".into(),
            "/projects".into()
        ]));
        assert!(!WorkspaceSearchSettings::valid_folders(&[String::new()]));
        assert!(!WorkspaceSearchSettings::valid_folders(&["x".repeat(4097)]));
        assert!(!WorkspaceSearchSettings::valid_folders(
            &(0..17).map(|n| format!("/{n}")).collect::<Vec<_>>()
        ));
    }
}
