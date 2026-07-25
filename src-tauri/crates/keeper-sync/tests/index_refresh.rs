use keeper_sync::git;

#[test]
fn refresh_index_stat_records_the_real_file_size() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(root)
        .status()
        .expect("init");
    std::fs::write(root.join("f.bin"), b"pointerish").expect("write small");
    std::process::Command::new("git")
        .args(["add", "f.bin"])
        .current_dir(root)
        .status()
        .expect("add");
    std::process::Command::new("git")
        .args([
            "-c",
            "user.email=t@e",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "x",
        ])
        .current_dir(root)
        .status()
        .expect("commit");

    // Grow the file, as materializing an LFS object does.
    std::fs::write(root.join("f.bin"), vec![3u8; 5000]).expect("grow");

    let repo = git::repo::open(root, false).expect("open");
    git::repo::refresh_index_stat(&repo, &[std::path::PathBuf::from("f.bin")]).expect("refresh");

    let out = std::process::Command::new("git")
        .args(["ls-files", "--debug", "f.bin"])
        .current_dir(root)
        .output()
        .expect("ls-files");
    let text = String::from_utf8_lossy(&out.stdout);
    println!("{text}");
    assert!(
        text.contains("size: 5000"),
        "the index must record the real size, got:\n{text}"
    );
}
