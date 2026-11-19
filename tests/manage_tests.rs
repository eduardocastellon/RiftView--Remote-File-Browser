//! End-to-end tests of rename, move, new folder, delete and move jobs
//! (Phase 7).
//!
//! Deletes go to a stand-in Trash folder in tests (see `TestHost`), never
//! to the real Trash.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use common::{TestHost, entry_names};
use riftview_api::filesystem::copying::copy_name;
use riftview_api::filesystem::trash::TrashBin;
use riftview_api::filesystem::{FileId, RelPath};
use riftview_api::jobs::NewMoveJob;
use riftview_api::jobs::moves::{MoveTask, recover_interrupted_jobs, spawn_move};
use serde_json::{Value, json};

// --- Helpers -------------------------------------------------------------------

async fn writable_host() -> TestHost {
    let host = TestHost::new().await;
    host.make_share_writable().await;
    host
}

fn id_for(share_id: &str, path: &str) -> String {
    FileId::new(share_id, RelPath::parse(path).unwrap()).encode()
}

async fn rename(host: &TestHost, path: &str, name: &str) -> (StatusCode, Value) {
    let uri = format!("/api/files/{}/rename", id_for(&host.share_id, path));
    host.send_json("POST", &uri, Some(json!({ "name": name })))
        .await
}

async fn move_to(host: &TestHost, path: &str, request: Value) -> (StatusCode, Value) {
    let uri = format!("/api/files/{}/move", id_for(&host.share_id, path));
    host.send_json("POST", &uri, Some(request)).await
}

async fn new_folder(host: &TestHost, parent: &str, name: &str) -> (StatusCode, Value) {
    let uri = format!("/api/shares/{}/directories", host.share_id);
    host.send_json("POST", &uri, Some(json!({ "path": parent, "name": name })))
        .await
}

async fn delete(host: &TestHost, path: &str) -> (StatusCode, Value) {
    let uri = format!("/api/files/{}", id_for(&host.share_id, path));
    host.send_json("DELETE", &uri, None).await
}

async fn job_status(host: &TestHost, job_id: &str) -> (StatusCode, Value) {
    host.send_json("GET", &format!("/api/jobs/{job_id}"), None)
        .await
}

/// Poll a job until it's no longer running.
async fn wait_for_job(host: &TestHost, job_id: &str) -> Value {
    for _ in 0..200 {
        let (status, job) = job_status(host, job_id).await;
        assert_eq!(status, StatusCode::OK, "{job}");
        if job["state"] != "running" {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("job {job_id} never finished");
}

/// Start a move job directly, as the move endpoint does when the
/// destination is on another disk. (Tests can't count on two disks, so this
/// exercises the copy path between two folders on the same one.)
fn start_copy_job(
    host: &TestHost,
    source_path: &str,
    destination_share_id: &str,
    destination_folder: &str,
    cancel_first: bool,
) -> String {
    let state = &host.state;
    let shares = state.shares().try_read().unwrap();
    let source_share = shares.get(&host.share_id).unwrap().clone();
    let destination_share = shares.get(destination_share_id).unwrap().clone();
    drop(shares);

    let (job, running) = state
        .jobs()
        .create(NewMoveJob {
            device_id: host.device.id.clone(),
            source_share_id: source_share.id.clone(),
            source_path: source_path.into(),
            destination_share_id: destination_share.id.clone(),
            destination_folder: destination_folder.into(),
        })
        .unwrap();
    if cancel_first {
        running.stop.stop();
    }

    let job_id = job.id.clone();
    spawn_move(
        state.clone(),
        MoveTask {
            job,
            running,
            source_share,
            source_path: RelPath::parse(source_path).unwrap(),
            destination_share,
            destination_folder: RelPath::parse(destination_folder).unwrap(),
            slot: state.transfer_limiter().try_start(&host.device.id).unwrap(),
        },
    );
    job_id
}

// --- Rename ----------------------------------------------------------------------

#[tokio::test]
async fn rename_a_file() {
    let host = writable_host().await;

    let (status, entry) = rename(&host, "/Documents/notes.txt", "todo.txt").await;
    assert_eq!(status, StatusCode::OK, "{entry}");
    assert_eq!(entry["name"], "todo.txt");
    assert_eq!(entry["path"], "/Documents/todo.txt");

    let docs = host.shared_path().join("Documents");
    assert_eq!(
        std::fs::read(docs.join("todo.txt")).unwrap(),
        b"hello world"
    );
    assert!(!docs.join("notes.txt").exists());

    // The returned ID works straight away.
    let (status, _) = host
        .get_json(&format!("/api/files/{}", entry["id"].as_str().unwrap()))
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn rename_never_replaces() {
    let host = writable_host().await;

    let (status, body) = rename(&host, "/Documents/notes.txt", "a.txt").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    let docs = host.shared_path().join("Documents");
    assert_eq!(std::fs::read(docs.join("a.txt")).unwrap(), b"a");
    assert_eq!(
        std::fs::read(docs.join("notes.txt")).unwrap(),
        b"hello world"
    );
}

#[tokio::test]
async fn rename_can_change_only_the_case() {
    let host = writable_host().await;

    let (status, entry) = rename(&host, "/Documents/notes.txt", "Notes.txt").await;
    assert_eq!(status, StatusCode::OK, "{entry}");

    let (_, listing) = host.browse("/Documents").await;
    let names = entry_names(&listing);
    assert!(names.contains(&"Notes.txt".to_string()), "{names:?}");
    assert!(!names.contains(&"notes.txt".to_string()), "{names:?}");
}

#[tokio::test]
async fn rename_a_folder() {
    let host = writable_host().await;
    let (status, entry) = rename(&host, "/Documents", "Docs").await;
    assert_eq!(status, StatusCode::OK, "{entry}");
    assert_eq!(entry["type"], "directory");
    assert!(host.shared_path().join("Docs/notes.txt").exists());
}

#[tokio::test]
async fn bad_new_names_are_refused() {
    let host = writable_host().await;

    for name in [
        "",
        "../x",
        "a/b",
        ".riftview-move-x.part",
        ".riftview-upload-x.part",
    ] {
        let (status, body) = rename(&host, "/report.pdf", name).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name:?}: {body}");
    }

    // The share's own folder can't be renamed, moved or deleted.
    let (status, _) = rename(&host, "/", "x").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = delete(&host, "/").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// --- Move ------------------------------------------------------------------------

#[tokio::test]
async fn move_within_a_share() {
    let host = writable_host().await;

    let (status, entry) = move_to(&host, "/Documents/notes.txt", json!({ "path": "/" })).await;
    assert_eq!(status, StatusCode::OK, "{entry}");
    assert_eq!(entry["path"], "/notes.txt");
    assert!(host.shared_path().join("notes.txt").exists());
    assert!(!host.shared_path().join("Documents/notes.txt").exists());

    // Moving a folder takes its contents.
    let (status, entry) = move_to(&host, "/Documents", json!({ "path": "/" })).await;
    assert_eq!(status, StatusCode::OK, "already at the root: nothing to do");
    assert_eq!(entry["path"], "/Documents");
}

#[tokio::test]
async fn move_refuses_clashes_and_loops() {
    let host = writable_host().await;
    std::fs::write(host.shared_path().join("a.txt"), b"root a").unwrap();

    let (status, _) = move_to(&host, "/Documents/a.txt", json!({ "path": "/" })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        std::fs::read(host.shared_path().join("a.txt")).unwrap(),
        b"root a"
    );

    let (status, _) = move_to(&host, "/Documents", json!({ "path": "/Documents" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "into itself");
    let (status, _) = move_to(
        &host,
        "/Documents",
        json!({ "path": "/Documents/Projects" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "into its own subfolder");

    let (status, _) = move_to(&host, "/Documents/a.txt", json!({ "path": "/Nope" })).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "destination must exist");
}

#[tokio::test]
async fn move_to_another_share() {
    let host = writable_host().await;
    let (second_id, second_folder) = host.add_second_share().await;

    let request = json!({ "share_id": second_id, "path": "/" });
    let (status, entry) = move_to(&host, "/Documents/notes.txt", request).await;
    // Both temp folders are on the same disk, so this is an instant rename.
    assert_eq!(status, StatusCode::OK, "{entry}");
    assert_eq!(entry["path"], "/notes.txt");
    assert!(second_folder.path().join("notes.txt").exists());
    assert!(!host.shared_path().join("Documents/notes.txt").exists());
}

#[tokio::test]
async fn move_needs_both_shares_writable() {
    let host = writable_host().await;
    let (second_id, _second_folder) = host.add_second_share().await;
    let uri = format!("/admin/shares/{second_id}");
    host.admin_json("PATCH", &uri, Some(json!({ "writable": false })))
        .await;

    let request = json!({ "share_id": second_id, "path": "/" });
    let (status, _) = move_to(&host, "/Documents/notes.txt", request).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(host.shared_path().join("Documents/notes.txt").exists());
}

// --- New folder ----------------------------------------------------------------------

#[tokio::test]
async fn create_a_folder() {
    let host = writable_host().await;

    let (status, entry) = new_folder(&host, "/Documents", "Invoices").await;
    assert_eq!(status, StatusCode::CREATED, "{entry}");
    assert_eq!(entry["type"], "directory");
    assert_eq!(entry["path"], "/Documents/Invoices");
    assert!(host.shared_path().join("Documents/Invoices").is_dir());

    let (status, _) = new_folder(&host, "/Documents", "Invoices").await;
    assert_eq!(status, StatusCode::CONFLICT, "already exists");
    let (status, _) = new_folder(&host, "/Documents", "notes.txt").await;
    assert_eq!(status, StatusCode::CONFLICT, "a file has that name");
    let (status, _) = new_folder(&host, "/Nope", "x").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "parent must exist");
    let (status, _) = new_folder(&host, "/", ".riftview-upload-x.part").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "reserved name");
}

// --- Delete --------------------------------------------------------------------------------

#[tokio::test]
async fn delete_files_and_whole_folders() {
    let host = writable_host().await;

    let (status, body) = delete(&host, "/report.pdf").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["deleted"], "trash");
    assert!(!host.shared_path().join("report.pdf").exists());

    // A folder with files and a subfolder in it.
    let (status, body) = delete(&host, "/Documents").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["deleted"], "trash");
    assert!(!host.shared_path().join("Documents").exists());

    // Both are in the Trash, not gone.
    let trashed = host.trashed_names();
    assert!(
        trashed.iter().any(|name| name.ends_with("-report.pdf")),
        "{trashed:?}"
    );
    assert!(
        trashed.iter().any(|name| name.ends_with("-Documents")),
        "{trashed:?}"
    );

    let (status, _) = delete(&host, "/Documents").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deleting_a_symlink_keeps_its_target() {
    let host = writable_host().await;
    let target = host.shared_path().join("Documents/notes.txt");
    std::os::unix::fs::symlink(&target, host.shared_path().join("link.txt")).unwrap();

    let (status, _) = delete(&host, "/link.txt").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!host.shared_path().join("link.txt").exists());
    assert!(target.exists(), "the file the link pointed to is untouched");
}

#[tokio::test]
async fn disks_without_a_trash_delete_permanently() {
    let host = TestHost::with_config(riftview_api::config::Config {
        auth_rate_limit_per_minute: 1000,
        trash_for_tests: Some(TrashBin::NoTrash),
        ..Default::default()
    })
    .await;
    host.make_share_writable().await;

    let (status, body) = delete(&host, "/Documents").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["deleted"], "permanent");
    assert!(!host.shared_path().join("Documents").exists());
}

#[tokio::test]
async fn other_trash_failures_delete_nothing() {
    // A Trash that can't receive anything (its folder doesn't exist).
    let host = TestHost::with_config(riftview_api::config::Config {
        auth_rate_limit_per_minute: 1000,
        trash_for_tests: Some(TrashBin::Folder("/nonexistent/riftview-trash".into())),
        ..Default::default()
    })
    .await;
    host.make_share_writable().await;

    let (status, body) = delete(&host, "/report.pdf").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("nothing was deleted")
    );
    assert!(
        host.shared_path().join("report.pdf").exists(),
        "never deleted permanently"
    );
}

// --- Permissions ------------------------------------------------------------------------------

#[tokio::test]
async fn each_operation_needs_its_permission() {
    let host = writable_host().await;
    let reader = host.pair_device("Reader", &["browse", "read"]).await;
    let token = host.log_in(&reader).await.unwrap();
    let notes = id_for(&host.share_id, "/Documents/notes.txt");

    let attempts = [
        (
            "POST",
            format!("/api/files/{notes}/rename"),
            json!({ "name": "x.txt" }),
        ),
        (
            "POST",
            format!("/api/files/{notes}/move"),
            json!({ "path": "/" }),
        ),
        (
            "POST",
            format!("/api/shares/{}/directories", host.share_id),
            json!({ "path": "/", "name": "New" }),
        ),
    ];
    for (method, uri, body) in attempts {
        let (status, _) = host
            .send_json_as(Some(&token), method, &uri, Some(body))
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}");
    }
    let (status, _) = host
        .send_json_as(Some(&token), "DELETE", &format!("/api/files/{notes}"), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    assert!(host.shared_path().join("Documents/notes.txt").exists());
}

#[tokio::test]
async fn read_only_shares_cannot_be_changed() {
    let host = TestHost::new().await; // Shares are read-only by default.

    let (status, _) = rename(&host, "/report.pdf", "x.pdf").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = delete(&host, "/report.pdf").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = new_folder(&host, "/", "New").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = move_to(&host, "/report.pdf", json!({ "path": "/Documents" })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// --- Move jobs (copying to another disk) ------------------------------------------------------

#[tokio::test]
async fn copy_job_moves_a_folder() {
    let host = writable_host().await;
    let (second_id, second_folder) = host.add_second_share().await;

    let job_id = start_copy_job(&host, "/Documents", &second_id, "/", false);
    let job = wait_for_job(&host, &job_id).await;

    assert_eq!(job["state"], "done", "{job}");
    assert_eq!(job["kind"], "move");
    assert_eq!(job["item"]["path"], "/Documents");
    assert_eq!(job["item"]["type"], "directory");
    assert_eq!(job["source"]["path"], "/Documents");
    assert_eq!(job["destination"]["share_id"], second_id.as_str());

    let moved = second_folder.path().join("Documents");
    assert_eq!(
        std::fs::read(moved.join("notes.txt")).unwrap(),
        b"hello world"
    );
    assert!(moved.join("Projects").is_dir());
    assert!(
        !host.shared_path().join("Documents").exists(),
        "original deleted"
    );
    assert!(
        !std::fs::read_dir(second_folder.path())
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".riftview")),
        "no temporary copy left"
    );
}

#[tokio::test]
async fn cancelled_copy_job_leaves_the_original() {
    let host = writable_host().await;
    let (second_id, second_folder) = host.add_second_share().await;

    let job_id = start_copy_job(&host, "/Documents", &second_id, "/", true);
    let job = wait_for_job(&host, &job_id).await;

    assert_eq!(job["state"], "cancelled", "{job}");
    assert!(host.shared_path().join("Documents/notes.txt").exists());
    assert_eq!(std::fs::read_dir(second_folder.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn copy_job_refuses_symlinks() {
    let host = writable_host().await;
    let (second_id, second_folder) = host.add_second_share().await;
    std::os::unix::fs::symlink("/etc", host.shared_path().join("Documents/link")).unwrap();

    let job_id = start_copy_job(&host, "/Documents", &second_id, "/", false);
    let job = wait_for_job(&host, &job_id).await;

    assert_eq!(job["state"], "failed", "{job}");
    assert!(job["error"].as_str().unwrap().contains("symlinks"));
    assert!(host.shared_path().join("Documents/notes.txt").exists());
    assert_eq!(std::fs::read_dir(second_folder.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn jobs_are_private_and_finished_jobs_cannot_be_cancelled() {
    let host = writable_host().await;
    let (second_id, _second_folder) = host.add_second_share().await;
    let job_id = start_copy_job(&host, "/report.pdf", &second_id, "/", false);
    wait_for_job(&host, &job_id).await;

    let other = host.pair_device("Other", &["browse", "read", "move"]).await;
    let token = host.log_in(&other).await.unwrap();
    let (status, _) = host
        .send_json_as(Some(&token), "GET", &format!("/api/jobs/{job_id}"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = host
        .send_json("DELETE", &format!("/api/jobs/{job_id}"), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "already finished");
}

#[tokio::test]
async fn interrupted_jobs_are_cleaned_up_after_a_restart() {
    let host = writable_host().await;
    let (second_id, second_folder) = host.add_second_share().await;

    // A job recorded as running, with a half-made copy, as a crash leaves it.
    let (job, _running) = host
        .state
        .jobs()
        .create(NewMoveJob {
            device_id: host.device.id.clone(),
            source_share_id: host.share_id.clone(),
            source_path: "/Documents".into(),
            destination_share_id: second_id.clone(),
            destination_folder: "/".into(),
        })
        .unwrap();
    let partial = second_folder.path().join(copy_name(&job.id));
    std::fs::create_dir(&partial).unwrap();
    std::fs::write(partial.join("half.txt"), b"half").unwrap();

    // While it's "running", its shares can't be removed.
    let (status, _) = host
        .admin_json("DELETE", &format!("/admin/shares/{second_id}"), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    recover_interrupted_jobs(&host.state).await;

    assert!(!partial.exists(), "partial copy removed");
    assert!(host.shared_path().join("Documents/notes.txt").exists());
    let (_, status_body) = job_status(&host, &job.id).await;
    assert_eq!(status_body["state"], "failed");
    assert!(status_body["error"].as_str().unwrap().contains("restart"));
}
