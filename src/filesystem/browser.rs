//! Bounded, non-recursive directory listing.

use serde::{Deserialize, Serialize};

use super::metadata::{Entry, EntryKind};
use super::roots::Share;
use super::{FsError, RelPath, blocking};

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SortKey {
    #[default]
    Name,
    Size,
    Modified,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    #[default]
    Asc,
    Desc,
}

#[derive(Debug, Clone)]
pub struct ListOptions {
    pub sort: SortKey,
    pub order: SortOrder,
    pub offset: usize,
    pub limit: usize,
    /// Refuse to sort directories larger than this.
    pub max_entries: usize,
}

#[derive(Debug, Serialize)]
pub struct DirListing {
    pub share_id: String,
    pub path: String,
    /// Total visible entries in the directory (before pagination).
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub entries: Vec<Entry>,
}

/// List one directory level. Directories sort before files.
pub async fn list_dir(share: Share, rel: RelPath, opts: ListOptions) -> Result<DirListing, FsError> {
    blocking(move || list_dir_blocking(&share, &rel, &opts)).await
}

fn list_dir_blocking(share: &Share, rel: &RelPath, opts: &ListOptions) -> Result<DirListing, FsError> {
    let dir = share.open_dir(rel)?;
    let mut entries = Vec::new();

    for (scanned, item) in dir.entries()?.enumerate() {
        if scanned >= opts.max_entries {
            return Err(FsError::TooManyEntries(opts.max_entries));
        }
        let Ok(item) = item else { continue };
        // Non-UTF-8 names cannot be addressed through the JSON API; skip.
        let Ok(name) = item.file_name().into_string() else { continue };
        let Ok(child) = rel.join_name(&name) else { continue };
        let Ok(ftype) = item.file_type() else { continue };

        let meta = if ftype.is_symlink() {
            // Follow the link *through the confined handle*: links that
            // escape the share or dangle fail here and are hidden.
            match dir.metadata(&name) {
                Ok(m) => m,
                Err(_) => continue,
            }
        } else {
            match item.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            }
        };
        if let Some(entry) = Entry::from_cap(&share.id, &child, &meta) {
            entries.push(entry);
        }
    }

    sort_entries(&mut entries, opts.sort, opts.order);
    let total = entries.len();
    let page: Vec<Entry> = entries.into_iter().skip(opts.offset).take(opts.limit).collect();

    Ok(DirListing {
        share_id: share.id.clone(),
        path: rel.to_string(),
        total,
        offset: opts.offset,
        limit: opts.limit,
        entries: page,
    })
}

fn sort_entries(entries: &mut [Entry], key: SortKey, order: SortOrder) {
    entries.sort_by(|a, b| {
        let dirs_first = (a.kind != EntryKind::Directory).cmp(&(b.kind != EntryKind::Directory));
        let by_name = || a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.name.cmp(&b.name));
        let primary = match key {
            SortKey::Name => by_name(),
            SortKey::Size => a.size.cmp(&b.size).then_with(by_name),
            SortKey::Modified => a.modified_ms.cmp(&b.modified_ms).then_with(by_name),
        };
        let primary = if order == SortOrder::Desc { primary.reverse() } else { primary };
        dirs_first.then(primary)
    });
}
