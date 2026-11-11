//! Tests that nothing outside a share can be reached
//! (planning.md §6.3 and §13.3).

mod common;

use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::{TestHost, entry_names};

/// Build a file ID by hand, the way an attacker would.
fn forged_file_id(share_id: &str, path: &str) -> String {
    URL_SAFE_NO_PAD.encode(format!("{share_id}:{path}"))
}

fn download_uri(file_id: &str) -> String {
    format!("/api/files/{file_id}/download")
}

#[tokio::test]
async fn traversal_in_browse_path_is_rejected() {
    let host = TestHost::new().await;
    let attacks = [
        "..",
        "/..",
        "/Documents/../..",
        "../../etc",
        "..%2F..",
        "/Documents/..\\..",
        "C:\\Windows",
    ];

    for path in attacks {
        let (status, body) = host.browse(path).await;
        let refused = status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND;
        assert!(refused, "{path}: {status} {body}");
        assert!(!body.to_string().contains("secret"), "{path}: {body}");
    }
}

#[tokio::test]
async fn absolute_host_path_is_treated_as_relative() {
    let host = TestHost::new().await;
    // A real absolute path is looked up *inside* the share, where it
    // doesn't exist.
    let absolute = host.outside_dir.path().to_string_lossy().into_owned();
    let (status, _) = host.browse(&absolute).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn forged_file_ids_cannot_escape() {
    let host = TestHost::new().await;
    let secret = host.outside_dir.path().join("secret.txt");
    let secret_absolute = secret.to_string_lossy().into_owned();

    for path in [
        "/../secret.txt",
        "/../../../../etc/passwd",
        &secret_absolute,
    ] {
        let file_id = forged_file_id(&host.share_id, path);
        let (status, body) = host.get_json(&download_uri(&file_id)).await;
        assert_ne!(status, StatusCode::OK, "{path}");
        assert!(!body.to_string().contains("TOP SECRET"));
    }
}

#[tokio::test]
async fn file_id_for_unknown_share_is_404() {
    let host = TestHost::new().await;
    let file_id = forged_file_id("shr_other", "/report.pdf");
    let (status, _) = host.get_json(&format!("/api/files/{file_id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_escapes_are_blocked() {
    use std::os::unix::fs::symlink;

    let host = TestHost::new().await;
    let shared = host.shared_path();
    let outside = host.outside_dir.path();

    symlink(outside.join("secret.txt"), shared.join("evil_file")).unwrap();
    symlink(outside, shared.join("evil_dir")).unwrap();
    symlink("/etc", shared.join("etc_link")).unwrap();
    symlink("../../../../../../", shared.join("Documents/up")).unwrap();

    // They don't appear in listings…
    let (_, listing) = host.browse("/").await;
    let names = entry_names(&listing);
    for escaping_link in ["evil_file", "evil_dir", "etc_link"] {
        assert!(
            !names.contains(&escaping_link.to_owned()),
            "{escaping_link} listed: {names:?}"
        );
    }

    // …and can't be browsed or downloaded even when named directly.
    for folder in ["/evil_dir", "/etc_link", "/Documents/up"] {
        assert_ne!(host.browse(folder).await.0, StatusCode::OK, "{folder}");
    }
    for file in [
        "/evil_file",
        "/evil_dir/secret.txt",
        "/Documents/up/etc/passwd",
    ] {
        let file_id = forged_file_id(&host.share_id, file);
        let (status, body) = host.get_json(&download_uri(&file_id)).await;
        assert_ne!(status, StatusCode::OK, "{file}");
        assert!(!body.to_string().contains("TOP SECRET"));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn symlinks_inside_the_share_still_work() {
    use std::os::unix::fs::symlink;

    let host = TestHost::new().await;
    symlink("Documents/notes.txt", host.shared_path().join("notes_link")).unwrap();

    let file_id = host.id_of("/", "notes_link").await;
    let (status, _) = host.get_json(&format!("/api/files/{file_id}")).await;
    assert_eq!(status, StatusCode::OK);
}

#[cfg(unix)]
#[tokio::test]
async fn named_pipe_is_not_served_and_does_not_hang() {
    let host = TestHost::new().await;

    let pipe_path = host.shared_path().join("pipe");
    let pipe_path_c = std::ffi::CString::new(pipe_path.to_string_lossy().as_bytes()).unwrap();
    let result = unsafe { libc::mkfifo(pipe_path_c.as_ptr(), 0o600) };
    assert_eq!(result, 0, "mkfifo failed");

    let (_, listing) = host.browse("/").await;
    assert!(!entry_names(&listing).contains(&"pipe".to_owned()));

    let file_id = forged_file_id(&host.share_id, "/pipe");
    let uri = download_uri(&file_id);
    let download = host.get_json(&uri);
    let (status, _) = tokio::time::timeout(std::time::Duration::from_secs(5), download)
        .await
        .expect("downloading a named pipe hung");
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn errors_do_not_leak_host_paths() {
    let host = TestHost::new().await;
    let host_path = host.shared_path().to_string_lossy().into_owned();

    let failing_requests = [
        format!("/api/shares/{}/files?path=/missing", host.share_id),
        download_uri(&forged_file_id(&host.share_id, "/Documents")),
    ];
    for uri in failing_requests {
        let (_, body) = host.get_json(&uri).await;
        assert!(!body.to_string().contains(&host_path), "{uri}: {body}");
    }
}

#[tokio::test]
async fn deleted_share_folder_reports_unavailable() {
    let host = TestHost::new().await;
    std::fs::remove_dir_all(host.shared_path()).unwrap();

    let uri = format!("/api/shares/{}/files", host.share_id);
    let (status, _) = host.get_json(&uri).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}
