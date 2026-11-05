//! Path-confinement tests (planning.md §6.3, §13.3).

mod common;

use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::TestHost;

fn forged_id(share: &str, path: &str) -> String {
    URL_SAFE_NO_PAD.encode(format!("{share}:{path}"))
}

#[tokio::test]
async fn traversal_in_browse_path_rejected() {
    let h = TestHost::new().await;
    for p in ["..", "/..", "/Documents/../..", "../../etc", "..%2F..", "/Documents/..\\..", "C:\\Windows"] {
        let (s, v) = h.browse(p).await;
        assert!(s == StatusCode::BAD_REQUEST || s == StatusCode::NOT_FOUND, "{p}: {s} {v}");
        assert!(!v.to_string().contains("secret"), "{p}: {v}");
    }
}

#[tokio::test]
async fn absolute_host_path_is_just_relative() {
    let h = TestHost::new().await;
    // An absolute host path is interpreted relative to the share root and
    // therefore does not exist there.
    let abs = h.outside.path().to_string_lossy().into_owned();
    let (s, _) = h.browse(&abs).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn forged_file_ids_cannot_escape() {
    let h = TestHost::new().await;
    let outside_abs = h.outside.path().join("secret.txt").to_string_lossy().into_owned();
    for p in ["/../secret.txt", "/../../../../etc/passwd", outside_abs.as_str()] {
        let id = forged_id(&h.share_id, p);
        let (s, v) = h.get(&format!("/api/files/{id}/download")).await;
        assert_ne!(s, StatusCode::OK, "{p}");
        assert!(!v.to_string().contains("TOP SECRET"));
    }
}

#[tokio::test]
async fn file_id_for_unknown_share_404() {
    let h = TestHost::new().await;
    let id = forged_id("shr_other", "/report.pdf");
    assert_eq!(h.get(&format!("/api/files/{id}")).await.0, StatusCode::NOT_FOUND);
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_escapes_are_blocked() {
    use std::os::unix::fs::symlink;
    let h = TestHost::new().await;
    symlink(h.outside.path().join("secret.txt"), h.root().join("evil_file")).unwrap();
    symlink(h.outside.path(), h.root().join("evil_dir")).unwrap();
    symlink("/etc", h.root().join("etc_link")).unwrap();
    symlink("../../../../../../", h.root().join("Documents/up")).unwrap();

    // Hidden from listings.
    let (_, v) = h.browse("/").await;
    let names: Vec<_> = v["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap().to_owned()).collect();
    for bad in ["evil_file", "evil_dir", "etc_link"] {
        assert!(!names.contains(&bad.to_owned()), "{bad} listed: {names:?}");
    }

    // Not browsable or downloadable even when named directly.
    assert_ne!(h.browse("/evil_dir").await.0, StatusCode::OK);
    assert_ne!(h.browse("/etc_link").await.0, StatusCode::OK);
    assert_ne!(h.browse("/Documents/up").await.0, StatusCode::OK);
    for p in ["/evil_file", "/evil_dir/secret.txt", "/Documents/up/etc/passwd"] {
        let (s, v) = h.get(&format!("/api/files/{}/download", forged_id(&h.share_id, p))).await;
        assert_ne!(s, StatusCode::OK, "{p}");
        assert!(!v.to_string().contains("TOP SECRET"));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn internal_symlinks_still_work() {
    use std::os::unix::fs::symlink;
    let h = TestHost::new().await;
    symlink("Documents/notes.txt", h.root().join("notes_link")).unwrap();
    let id = h.id_of("/", "notes_link").await;
    let (s, _) = h.get(&format!("/api/files/{id}")).await;
    assert_eq!(s, StatusCode::OK);
}

#[cfg(unix)]
#[tokio::test]
async fn fifo_is_not_served_and_does_not_hang() {
    let h = TestHost::new().await;
    let fifo = h.root().join("pipe");
    let c = std::ffi::CString::new(fifo.to_string_lossy().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    let (_, v) = h.browse("/").await;
    assert!(!v.to_string().contains("\"pipe\""));
    let id = forged_id(&h.share_id, "/pipe");
    let res = tokio::time::timeout(std::time::Duration::from_secs(5), h.get(&format!("/api/files/{id}/download"))).await;
    let (s, _) = res.expect("download of FIFO hung");
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn errors_do_not_leak_host_paths() {
    let h = TestHost::new().await;
    let root = h.root().to_string_lossy().into_owned();
    for uri in [
        format!("/api/shares/{}/files?path=/missing", h.share_id),
        format!("/api/files/{}/download", forged_id(&h.share_id, "/Documents")),
    ] {
        let (_, v) = h.get(&uri).await;
        assert!(!v.to_string().contains(&root), "{uri}: {v}");
    }
}

#[tokio::test]
async fn removed_share_root_reports_unavailable() {
    let h = TestHost::new().await;
    let id = h.share_id.clone();
    std::fs::remove_dir_all(h.root()).unwrap();
    let (s, _) = h.get(&format!("/api/shares/{id}/files")).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
}
