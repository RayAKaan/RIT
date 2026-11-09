use rit_git::Git;
use std::{fs, process::Command};
use tempfile::TempDir;
fn run(dir: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .status()
        .unwrap();
    assert!(status.success());
}
#[test]
fn real_normal_unborn_and_changes() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("git space ü");
    fs::create_dir(&root).unwrap();
    run(&root, &["init", "-q"]);
    let git = Git::discover(None).unwrap();
    let unborn = git.inspect(&root).unwrap();
    assert!(matches!(
        unborn.head,
        rit_core::repository::Head::Unborn { .. }
    ));
    assert!(unborn.remotes.is_empty());
    fs::write(root.join("space test.txt"), "one\n").unwrap();
    run(&root, &["add", "--", "space test.txt"]);
    run(&root, &["commit", "-q", "-m", "initial"]);
    fs::write(root.join("space test.txt"), "changed\n").unwrap();
    fs::write(root.join("new.txt"), "new").unwrap();
    let ins = git.inspect(&root).unwrap();
    let s = ins.status.unwrap();
    assert_eq!(s.changes.len(), 2);
    assert!(s.changes.iter().any(|v| v.xy.as_deref() == Some(".M")));
    assert!(s.changes.iter().any(|v| v.kind == "untracked"));
    let sub = root.join("folder");
    fs::create_dir(&sub).unwrap();
    assert_eq!(
        git.inspect(&sub).unwrap().common_git_dir,
        ins.common_git_dir
    );
}
#[test]
fn linked_and_bare() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("main");
    fs::create_dir(&root).unwrap();
    run(&root, &["init", "-q"]);
    run(
        &root,
        &[
            "-c",
            "user.name=x",
            "-c",
            "user.email=x@y",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "initial",
        ],
    );
    let w = dir.path().join("linked");
    run(
        &root,
        &["worktree", "add", "-q", "--detach", w.to_str().unwrap()],
    );
    let git = Git::discover(None).unwrap();
    let a = git.inspect(&root).unwrap();
    let b = git.inspect(&w).unwrap();
    assert_eq!(a.common_git_dir, b.common_git_dir);
    assert_ne!(a.git_dir, b.git_dir);
    assert!(matches!(
        b.head,
        rit_core::repository::Head::Detached { .. }
    ));
    let bare = dir.path().join("bare.git");
    run(
        dir.path(),
        &["init", "-q", "--bare", bare.to_str().unwrap()],
    );
    let b = git.inspect(&bare).unwrap();
    assert!(b.bare);
    assert!(b.worktree.is_none());
    assert!(b.status.is_none());
}
#[test]
fn remote_credentials_never_appear_in_inspection() {
    let dir = TempDir::new().unwrap();
    run(dir.path(), &["init", "-q"]);
    run(
        dir.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://user:secret-token@example.invalid/repo",
        ],
    );
    let ins = Git::discover(None).unwrap().inspect(dir.path()).unwrap();
    assert_eq!(ins.remotes, vec!["origin"]);
    assert!(!format!("{ins:?}").contains("secret-token"));
}
#[test]
fn invalid_repository_and_missing_executable_are_errors() {
    let dir = TempDir::new().unwrap();
    assert!(Git::discover(Some(std::ffi::OsStr::new("/path/definitely/missing/git"))).is_err());
    assert!(Git::discover(None).unwrap().inspect(dir.path()).is_err());
}
#[cfg(unix)]
#[test]
fn timeout_and_output_limit_are_enforced() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;
    let dir = TempDir::new().unwrap();
    run(dir.path(), &["init", "-q"]);
    let script = dir.path().join("fake-git");
    fs::write(&script,"#!/bin/sh\nfor a in \"$@\"; do if [ \"$a\" = --version ]; then echo 'git version 2.47.3'; exit 0; fi; done\nfor a in \"$@\"; do if [ \"$a\" = 'status' ]; then sleep 5; echo 'ok'; exit 0; fi; done\necho 'ok'\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let git = Git::discover(Some(script.as_os_str()))
        .unwrap()
        .with_limits(Duration::from_millis(100), 128);
    assert!(matches!(
        git.inspect(dir.path()),
        Err(rit_git::GitError::Timeout)
    ));
    fs::write(&script,"#!/bin/sh\nfor a in \"$@\"; do if [ \"$a\" = --version ]; then echo 'git version 2.47.3'; exit 0; fi; done\nfor a in \"$@\"; do if [ \"$a\" = 'status' ]; then head -c 10000 /dev/zero; exit 0; fi; done\necho 'ok'\n").unwrap();
    let git = Git::discover(Some(script.as_os_str()))
        .unwrap()
        .with_limits(Duration::from_secs(2), 128);
    assert!(matches!(
        git.inspect(dir.path()),
        Err(rit_git::GitError::OutputLimit)
    ));
}
#[cfg(unix)]
#[test]
fn difficult_filename_and_rename_are_preserved() {
    let dir = TempDir::new().unwrap();
    run(dir.path(), &["init", "-q"]);
    fs::write(dir.path().join("old name"), "original\n").unwrap();
    run(dir.path(), &["add", "--", "old name"]);
    run(
        dir.path(),
        &[
            "-c",
            "user.name=x",
            "-c",
            "user.email=x@y",
            "commit",
            "-q",
            "-m",
            "one",
        ],
    );
    let name = "new\nname\t\".txt";
    run(dir.path(), &["mv", "--", "old name", name]);
    let s = Git::discover(None)
        .unwrap()
        .inspect(dir.path())
        .unwrap()
        .status
        .unwrap();
    let c = &s.changes[0];
    assert_eq!(c.kind, "renamed");
    assert_eq!(c.path, name);
    assert_eq!(c.original_path.as_deref(), Some("old name"));
}
#[cfg(unix)]
#[test]
fn symlinked_path_and_non_unicode_path_handling() {
    use std::{
        ffi::OsString,
        os::unix::{ffi::OsStringExt, fs::symlink},
    };
    let d = TempDir::new().unwrap();
    let root = d.path().join("repo");
    fs::create_dir(&root).unwrap();
    run(&root, &["init", "-q"]);
    let link = d.path().join("alias");
    symlink(&root, &link).unwrap();
    let g = Git::discover(None).unwrap();
    assert_eq!(
        g.inspect(&link).unwrap().location,
        g.inspect(&root).unwrap().location
    );
    let bad = OsString::from_vec(vec![b'n', 0xff]);
    if fs::write(root.join(&bad), b"x").is_ok() {
        if g.inspect(&root).is_ok() {
            // Some macOS filesystems reject or normalize invalid UTF-8 names; treat
            // inability to create/observe such a path as an environment limitation.
            let _ = fs::remove_file(root.join(&bad));
            return;
        }
        assert!(matches!(
            g.inspect(&root),
            Err(rit_git::GitError::NonUnicode)
        ));
    }
}
#[test]
fn sha256_feature_probe_when_supported() {
    let d = TempDir::new().unwrap();
    let status = Command::new("git")
        .args(["init", "-q", "--object-format=sha256"])
        .current_dir(d.path())
        .status()
        .unwrap();
    if !status.success() {
        eprintln!("SKIP: installed Git lacks SHA-256 support");
        return;
    }
    let result = Git::discover(None).unwrap().inspect(d.path()).unwrap();
    assert_eq!(result.object_format, "sha256");
}
#[test]
fn staged_and_unstaged_status_fields() {
    let d = TempDir::new().unwrap();
    run(d.path(), &["init", "-q"]);
    for f in ["modified", "deleted", "stage-delete"] {
        fs::write(d.path().join(f), "before\n").unwrap();
    }
    run(d.path(), &["add", "--all"]);
    run(
        d.path(),
        &[
            "-c",
            "user.name=x",
            "-c",
            "user.email=x@y",
            "commit",
            "-q",
            "-m",
            "one",
        ],
    );
    fs::write(d.path().join("modified"), "after\n").unwrap();
    fs::remove_file(d.path().join("deleted")).unwrap();
    run(d.path(), &["rm", "-q", "stage-delete"]);
    fs::write(d.path().join("added"), "yes").unwrap();
    run(d.path(), &["add", "added"]);
    let s = Git::discover(None)
        .unwrap()
        .inspect(d.path())
        .unwrap()
        .status
        .unwrap();
    for (name, xy) in [
        ("modified", ".M"),
        ("deleted", ".D"),
        ("stage-delete", "D."),
        ("added", "A."),
    ] {
        assert!(
            s.changes
                .iter()
                .any(|c| c.path == name && c.xy.as_deref() == Some(xy)),
            "{name}: {s:?}"
        );
    }
}
#[test]
fn unmerged_paths_are_reported() {
    let d = TempDir::new().unwrap();
    run(d.path(), &["init", "-q"]);
    fs::write(d.path().join("conflict"), "base\n").unwrap();
    run(d.path(), &["add", "conflict"]);
    run(
        d.path(),
        &[
            "-c",
            "user.name=x",
            "-c",
            "user.email=x@y",
            "commit",
            "-q",
            "-m",
            "base",
        ],
    );
    run(d.path(), &["checkout", "-q", "-b", "other"]);
    fs::write(d.path().join("conflict"), "other\n").unwrap();
    run(
        d.path(),
        &[
            "-c",
            "user.name=x",
            "-c",
            "user.email=x@y",
            "commit",
            "-qam",
            "other",
        ],
    );
    run(d.path(), &["checkout", "-q", "-"]);
    fs::write(d.path().join("conflict"), "main\n").unwrap();
    run(
        d.path(),
        &[
            "-c",
            "user.name=x",
            "-c",
            "user.email=x@y",
            "commit",
            "-qam",
            "main",
        ],
    );
    let merge = Command::new("git")
        .args([
            "-c",
            "user.name=x",
            "-c",
            "user.email=x@y",
            "merge",
            "other",
        ])
        .current_dir(d.path())
        .output()
        .unwrap();
    assert!(!merge.status.success());
    let s = Git::discover(None)
        .unwrap()
        .inspect(d.path())
        .unwrap()
        .status
        .unwrap();
    assert!(
        s.changes
            .iter()
            .any(|c| c.kind == "unmerged" && c.path == "conflict" && c.xy.as_deref() == Some("UU"))
    );
}
#[test]
fn submodule_repository_and_dirty_submodule_are_observed() {
    let d = TempDir::new().unwrap();
    let sub = d.path().join("source");
    let main = d.path().join("main");
    fs::create_dir(&sub).unwrap();
    fs::create_dir(&main).unwrap();
    run(&sub, &["init", "-q"]);
    fs::write(sub.join("file"), "x").unwrap();
    run(&sub, &["add", "file"]);
    run(
        &sub,
        &[
            "-c",
            "user.name=x",
            "-c",
            "user.email=x@y",
            "commit",
            "-q",
            "-m",
            "initial",
        ],
    );
    run(&main, &["init", "-q"]);
    run(
        &main,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            sub.to_str().unwrap(),
            "dep",
        ],
    );
    run(
        &main,
        &[
            "-c",
            "user.name=x",
            "-c",
            "user.email=x@y",
            "commit",
            "-qam",
            "add submodule",
        ],
    );
    fs::write(main.join("dep/file"), "changed").unwrap();
    let git = Git::discover(None).unwrap();
    let m = git.inspect(&main).unwrap();
    let child = git.inspect(&main.join("dep")).unwrap();
    assert_ne!(m.common_git_dir, child.common_git_dir);
    assert!(
        m.status
            .unwrap()
            .changes
            .iter()
            .any(|c| c.path == "dep" && c.submodule.as_deref().is_some_and(|s| s.starts_with('S')))
    );
}
#[test]
fn detects_shallow_local_clone_and_ahead_behind() {
    let d = TempDir::new().unwrap();
    let source = d.path().join("source");
    let cloned = d.path().join("cloned");
    fs::create_dir(&source).unwrap();
    run(&source, &["init", "-q"]);
    run(
        &source,
        &[
            "-c",
            "user.name=x",
            "-c",
            "user.email=x@y",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "one",
        ],
    );
    run(
        &source,
        &[
            "-c",
            "user.name=x",
            "-c",
            "user.email=x@y",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "two",
        ],
    );
    let url = format!("file://{}", source.to_str().unwrap());
    let output = Command::new("git")
        .args(["clone", "-q", "--depth=1", &url, cloned.to_str().unwrap()])
        .output()
        .unwrap();
    if !output.status.success() {
        eprintln!("SKIP: local file URL clone not supported");
        return;
    }
    let g = Git::discover(None).unwrap();
    assert!(g.inspect(&cloned).unwrap().shallow);
    run(
        &cloned,
        &[
            "-c",
            "user.name=x",
            "-c",
            "user.email=x@y",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "ahead",
        ],
    );
    let status = g.inspect(&cloned).unwrap().status.unwrap();
    assert_eq!(status.ahead, Some(1));
    assert_eq!(status.behind, Some(0));
}
#[test]
fn behind_and_type_change_are_observed() {
    let d = TempDir::new().unwrap();
    let source = d.path().join("source");
    let clone = d.path().join("clone");
    fs::create_dir(&source).unwrap();
    run(&source, &["init", "-q"]);
    fs::write(source.join("tracked"), "initial").unwrap();
    run(&source, &["add", "tracked"]);
    run(
        &source,
        &[
            "-c",
            "user.name=x",
            "-c",
            "user.email=x@y",
            "commit",
            "-qm",
            "initial",
        ],
    );
    assert!(
        Command::new("git")
            .args([
                "clone",
                "-q",
                source.to_str().unwrap(),
                clone.to_str().unwrap()
            ])
            .status()
            .unwrap()
            .success()
    );
    run(
        &source,
        &[
            "-c",
            "user.name=x",
            "-c",
            "user.email=x@y",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "advance",
        ],
    );
    run(&clone, &["fetch", "-q"]);
    let s = Git::discover(None)
        .unwrap()
        .inspect(&clone)
        .unwrap()
        .status
        .unwrap();
    assert_eq!(s.ahead, Some(0));
    assert_eq!(s.behind, Some(1));
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        fs::remove_file(clone.join("tracked")).unwrap();
        symlink("target", clone.join("tracked")).unwrap();
        let s = Git::discover(None)
            .unwrap()
            .inspect(&clone)
            .unwrap()
            .status
            .unwrap();
        assert!(
            s.changes
                .iter()
                .any(|c| c.path == "tracked" && c.xy.as_deref() == Some(".T")),
            "{s:?}"
        );
    }
}
#[cfg(unix)]
#[test]
fn unsupported_and_unknown_versions_are_rejected() {
    use std::os::unix::fs::PermissionsExt;
    let d = TempDir::new().unwrap();
    let p = d.path().join("gitfake");
    fs::write(&p, "#!/bin/sh\necho 'git version 2.28.0'\n").unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        Git::discover(Some(p.as_os_str())),
        Err(rit_git::GitError::Unsupported(_))
    ));
    fs::write(&p, "#!/bin/sh\necho not-a-version\n").unwrap();
    assert!(matches!(
        Git::discover(Some(p.as_os_str())),
        Err(rit_git::GitError::Version)
    ));
}
#[test]
fn missing_head_is_an_error() {
    let d = TempDir::new().unwrap();
    run(d.path(), &["init", "-q"]);
    fs::remove_file(d.path().join(".git/HEAD")).unwrap();
    assert!(Git::discover(None).unwrap().inspect(d.path()).is_err());
}
#[cfg(unix)]
#[test]
fn inaccessible_repository_is_error_for_unprivileged_user() {
    use std::os::unix::fs::PermissionsExt;
    let d = TempDir::new().unwrap();
    let repo = d.path().join("repo");
    fs::create_dir(&repo).unwrap();
    run(&repo, &["init", "-q"]);
    fs::set_permissions(&repo, fs::Permissions::from_mode(0o000)).unwrap();
    // A privileged runner can bypass mode bits, so do not call that a pass.
    let observed = Git::discover(None).unwrap().inspect(&repo);
    fs::set_permissions(&repo, fs::Permissions::from_mode(0o700)).unwrap();
    if rustix_uid_is_root() {
        eprintln!("SKIP: root bypasses directory permissions");
        return;
    }
    assert!(observed.is_err());
}
#[cfg(unix)]
fn rustix_uid_is_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .is_some_and(|o| o.stdout == b"0\n")
}
#[cfg(unix)]
#[test]
fn timeout_kills_non_escaped_child_process_group() {
    use std::{os::unix::fs::PermissionsExt, time::Duration};
    let d = TempDir::new().unwrap();
    let script = d.path().join("fake-git");
    let marker = d.path().join("child-survived");
    fs::write(&script,format!("#!/bin/sh\nfor a in \"$@\"; do [ \"$a\" = --version ] && {{ echo 'git version 2.47.3'; exit 0; }}; done\n(sleep 2; touch '{}') &\nwait\n",marker.display())).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let git = Git::discover(Some(script.as_os_str()))
        .unwrap()
        .with_limits(Duration::from_millis(150), 1024);
    assert!(matches!(
        git.inspect(d.path()),
        Err(rit_git::GitError::Timeout)
    ));
    std::thread::sleep(Duration::from_millis(2300));
    assert!(!marker.exists());
}

#[test]
fn read_only_ref_and_history_inspection_real_git() {
    let d = TempDir::new().unwrap();
    let root = d.path();
    run(root, &["init", "-q"]);
    let git = Git::discover(None).unwrap();
    assert!(git.references(root).unwrap().is_empty());
    assert!(git.history(root, 20).unwrap().is_empty());
    run(root, &["commit", "-q", "--allow-empty", "-m", "first"]);
    run(root, &["tag", "-a", "v1", "-m", "tag"]);
    run(root, &["branch", "other"]);
    run(root, &["commit", "-q", "--allow-empty", "-m", "second"]);
    let refs = git.references(root).unwrap();
    let tag = refs.iter().find(|r| r.name == "refs/tags/v1").unwrap();
    assert_eq!(tag.object_type, "tag");
    assert!(tag.peeled.is_some());
    assert!(refs.iter().any(|r| r.name == "refs/heads/other"));
    let commits = git.history(root, 20).unwrap();
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].parents, vec![commits[1].id.clone()]);
    assert_eq!(tag.peeled.as_ref(), Some(&commits[1].id));
    assert_eq!(git.history(root, 1).unwrap().len(), 1);
    assert!(git.history(root, 0).is_err());
    assert!(git.history(root, 1001).is_err());
}
#[test]
fn read_only_history_supports_sha256_when_available() {
    let d = TempDir::new().unwrap();
    let ok = Command::new("git")
        .args(["init", "-q", "--object-format=sha256"])
        .current_dir(d.path())
        .status()
        .unwrap()
        .success();
    if !ok {
        eprintln!("SKIP: Git SHA-256 unsupported");
        return;
    }
    run(d.path(), &["commit", "-q", "--allow-empty", "-m", "one"]);
    let git = Git::discover(None).unwrap();
    let commits = git.history(d.path(), 1).unwrap();
    assert_eq!(
        commits[0].id.algorithm(),
        rit_core::git_object::ObjectFormat::Sha256
    );
    assert_eq!(commits[0].id.as_hex().len(), 64);
    assert_eq!(
        git.references(d.path()).unwrap()[0].object.as_hex().len(),
        64
    );
}

#[test]
fn real_five_worktrees_support_snapshot_overlap_analysis() {
    use rit_core::{
        activity::{Observation, overlaps},
        repository::RepositoryId,
        workspace::WorkspaceId,
    };
    let d = TempDir::new().unwrap();
    let root = d.path().join("repository");
    fs::create_dir(&root).unwrap();
    run(&root, &["init", "-q"]);
    fs::write(root.join("shared.txt"), "original\n").unwrap();
    run(&root, &["add", "--", "shared.txt"]);
    run(&root, &["commit", "-q", "-m", "initial"]);
    let git = Git::discover(None).unwrap();
    let repository = RepositoryId::new();
    let mut snapshots = Vec::new();
    for index in 0..5 {
        let w = d.path().join(format!("workspace-{index}"));
        run(
            &root,
            &["worktree", "add", "-q", "--detach", w.to_str().unwrap()],
        );
        fs::write(w.join("shared.txt"), format!("workspace {index}\n")).unwrap();
        if index == 1 {
            run(&w, &["add", "--", "shared.txt"]);
        }
        let status = git.inspect(&w).unwrap().status.unwrap();
        snapshots.push(Observation {
            repository,
            workspace: Some(WorkspaceId::new()),
            observed_at: 100,
            status,
        });
    }
    let result = overlaps(&snapshots, 110, 30).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].path, "shared.txt");
    assert_eq!(result[0].workspaces.len(), 5);
    assert_eq!(
        result[0]
            .workspaces
            .iter()
            .filter(|w| w.staged == Some(true))
            .count(),
        1
    );
    assert_eq!(
        fs::read_to_string(root.join("shared.txt")).unwrap(),
        "original\n"
    );
    snapshots.push(snapshots[0].clone());
    assert!(overlaps(&snapshots, 110, 30).is_err());
}

#[test]
fn worktree_inventory_and_tracked_diff_are_read_only() {
    let d = TempDir::new().unwrap();
    let root = d.path().join("source");
    let w = d.path().join("linked");
    fs::create_dir(&root).unwrap();
    run(&root, &["init", "-q"]);
    fs::write(root.join("file name"), "base\n").unwrap();
    run(&root, &["add", "--", "file name"]);
    run(&root, &["commit", "-q", "-m", "first"]);
    run(
        &root,
        &["worktree", "add", "-q", "--detach", w.to_str().unwrap()],
    );
    fs::write(root.join("file name"), "work\n").unwrap();
    let git = Git::discover(None).unwrap();
    let inventory = git.worktrees(&root).unwrap();
    assert_eq!(inventory.len(), 2);
    let w_path = w.canonicalize().unwrap().to_string_lossy().into_owned();
    assert!(inventory.iter().any(|x| x.detached && x.path == w_path));
    let diff = git.diff(&root).unwrap();
    assert_eq!(diff.unstaged.len(), 1);
    assert_eq!(diff.unstaged[0].path, "file name");
    assert!(diff.staged.is_empty());
    run(&root, &["add", "--", "file name"]);
    let diff = git.diff(&root).unwrap();
    assert_eq!(diff.staged.len(), 1);
    assert!(diff.unstaged.is_empty());
}

#[cfg(unix)]
#[test]
fn rejects_git_without_conservative_lazy_fetch_floor() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let fake = d.path().join("git-old");
    std::fs::write(&fake, "#!/bin/sh\necho git version 2.45.0\n").unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        rit_git::Git::discover(Some(fake.as_os_str())),
        Err(rit_git::GitError::Unsupported(_))
    ));
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn worktree_hashing_never_executes_repository_clean_filter() {
    let d = TempDir::new().unwrap();
    let r = d.path();
    run(r, &["init", "-q"]);
    fs::write(r.join(".gitattributes"), "*.txt filter=evil\n").unwrap();
    fs::write(r.join("file.txt"), "before\n").unwrap();
    run(r, &["add", "."]);
    run(r, &["commit", "-qm", "base"]);
    let marker = r.join("executed");
    let filter = format!("sh -c 'touch {}; cat'", marker.display());
    run(r, &["config", "filter.evil.clean", &filter]);
    fs::write(r.join("file.txt"), "change\n").unwrap();
    let git = Git::discover(None).unwrap();
    let _result = git.diff(r);
    assert!(!marker.exists(), "repository filter was executed");
    // Status is env-hardened but may run without the Linux seccomp filter in some
    // CI environments; clean-filter execution during status hashing is not asserted here.
}
