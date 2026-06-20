use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RomSource {
    pub id: String,
    pub name: String,
    pub kind: SourceKind,
    pub base_url: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    DirectoryIndex,
    JsonApi,
    Mirror,
    Custom,
}

pub fn built_in() -> Vec<RomSource> {
    vec![
        RomSource {
            id: "myrient".into(),
            name: "Myrient (No-Intro/Redump mirror)".into(),
            kind: SourceKind::DirectoryIndex,
            base_url: "https://myrient.erista.me/files/".into(),
            notes: Some("public mirror; index per system".into()),
        },
        RomSource {
            id: "archive-org".into(),
            name: "Internet Archive".into(),
            kind: SourceKind::Mirror,
            base_url: "https://archive.org/details/".into(),
            notes: Some("varied collections; check item description".into()),
        },
        RomSource {
            id: "vimms-lair".into(),
            name: "Vimm's Lair".into(),
            kind: SourceKind::Mirror,
            base_url: "https://vimm.net/vault/".into(),
            notes: None,
        },
        RomSource {
            id: "user-defined".into(),
            name: "User-defined URL".into(),
            kind: SourceKind::Custom,
            base_url: "".into(),
            notes: Some("paste any direct URL; agent will fetch as-is".into()),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_sources_non_empty_and_unique() {
        let s = built_in();
        assert!(!s.is_empty());
        let mut ids: Vec<_> = s.iter().map(|x| x.id.clone()).collect();
        ids.sort();
        let mut dedup = ids.clone();
        dedup.dedup();
        assert_eq!(ids.len(), dedup.len(), "source ids must be unique");
    }

    #[test]
    fn user_defined_has_empty_base() {
        let s = built_in();
        let u = s.iter().find(|s| s.id == "user-defined").unwrap();
        assert!(u.base_url.is_empty());
    }
}
