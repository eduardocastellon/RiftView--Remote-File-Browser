//! Listing the contents of one folder (never recursive, always bounded).

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use super::metadata::{Entry, EntryKind};
use super::roots::Share;
use super::{FsError, RelPath, run_blocking};

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

    /// How many entries to skip (for paging).
    pub offset: usize,

    /// How many entries to return at most (page size).
    pub limit: usize,

    /// Refuse folders with more entries than this, instead of loading
    /// them all into memory to sort.
    pub max_entries: usize,
}

/// One page of a folder listing.
#[derive(Debug, Serialize)]
pub struct DirListing {
    pub share_id: String,
    pub path: String,

    /// Number of visible entries in the whole folder, before paging.
    pub total: usize,

    pub offset: usize,
    pub limit: usize,
    pub entries: Vec<Entry>,
}

/// List one folder. Folders are always listed before files.
pub async fn list_dir(
    share: Share,
    path: RelPath,
    options: ListOptions,
) -> Result<DirListing, FsError> {
    run_blocking(move || list_dir_blocking(&share, &path, &options)).await
}

fn list_dir_blocking(
    share: &Share,
    path: &RelPath,
    options: &ListOptions,
) -> Result<DirListing, FsError> {
    let dir = share.open_dir(path)?;
    let mut entries = Vec::new();

    for (count_so_far, dir_entry) in dir.entries()?.enumerate() {
        if count_so_far >= options.max_entries {
            return Err(FsError::TooManyEntries(options.max_entries));
        }

        // Entries we can't read are skipped rather than failing the whole
        // listing.
        let Ok(dir_entry) = dir_entry else { continue };

        // Names that aren't valid UTF-8 can't be sent through the JSON
        // API, so skip them.
        let Ok(name) = dir_entry.file_name().into_string() else {
            continue;
        };
        let Ok(child_path) = path.join_name(&name) else {
            continue;
        };
        let Ok(file_type) = dir_entry.file_type() else {
            continue;
        };

        let metadata = if file_type.is_symlink() {
            // Follow the symlink *through our confined handle*. Links that
            // point outside the share, or point nowhere, fail here and are
            // simply not shown.
            dir.metadata(&name)
        } else {
            dir_entry.metadata()
        };
        let Ok(metadata) = metadata else { continue };

        if let Some(entry) = Entry::from_cap_metadata(&share.id, &child_path, &metadata) {
            entries.push(entry);
        }
    }

    sort_entries(&mut entries, options.sort, options.order);

    let total = entries.len();
    let page = entries
        .into_iter()
        .skip(options.offset)
        .take(options.limit)
        .collect();

    Ok(DirListing {
        share_id: share.id.clone(),
        path: path.to_string(),
        total,
        offset: options.offset,
        limit: options.limit,
        entries: page,
    })
}

fn sort_entries(entries: &mut [Entry], key: SortKey, order: SortOrder) {
    entries.sort_by(|a, b| {
        // Folders always come first, regardless of sort order.
        let folders_first = is_file(a).cmp(&is_file(b));
        if folders_first != Ordering::Equal {
            return folders_first;
        }

        let by_chosen_key = match key {
            SortKey::Name => compare_names(a, b),
            SortKey::Size => a.size.cmp(&b.size).then_with(|| compare_names(a, b)),
            SortKey::Modified => a
                .modified_ms
                .cmp(&b.modified_ms)
                .then_with(|| compare_names(a, b)),
        };

        match order {
            SortOrder::Asc => by_chosen_key,
            SortOrder::Desc => by_chosen_key.reverse(),
        }
    });
}

fn is_file(entry: &Entry) -> bool {
    entry.kind != EntryKind::Directory
}

/// Case-insensitive name order, with exact case as a tie-breaker so the
/// result is always stable.
fn compare_names(a: &Entry, b: &Entry) -> Ordering {
    let case_insensitive = a.name.to_lowercase().cmp(&b.name.to_lowercase());
    case_insensitive.then_with(|| a.name.cmp(&b.name))
}
