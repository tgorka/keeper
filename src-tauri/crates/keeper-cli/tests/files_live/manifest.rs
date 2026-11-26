use super::*;

#[cfg(target_os = "linux")]
#[test]
#[ignore = "needs two scratch devices and Linux network namespaces"]
fn offline_writers_converge_on_both_versions() {
    let cfg = live();
    let a = Device::new(&cfg, "EDITOR");
    let b = Device::new(&cfg, "PEER");
    assert!(a.client != b.client);
    let stem = unique("conflict");
    let name = format!("{stem}.txt");
    let original = a.put(&name, b"base");
    let edit = |d: &Device<'_>, bytes: &[u8]| {
        d.get(&name, b"base");
        let file = d.body(bytes);
        let queued = d.call(
            &["put", "--file", &file, "--drive", &cfg.drive, &name],
            4,
            true,
        );
        assert!(queued["outcome"] == "queued");
        assert!(queued["content_id"] == hash_file(Path::new(&file)).1);
        queued
    };
    let x = edit(&a, b"first");
    let y = edit(&b, b"second");
    let list = ["ls", "--drive", &cfg.drive];
    let first = a.ok(&list);
    let [accepted] = rows(&first, "flushed") else {
        panic!("expected one flush")
    };
    assert!(accepted["outcome"] == "accepted");
    assert!(text(accepted, "content_id") == text(&x, "content_id"));
    let second = b.ok(&list);
    let [copy] = rows(&second, "flushed") else {
        panic!("expected one conflict flush")
    };
    assert!(copy["outcome"] == "conflict_copy");
    let original_id = text(&original, "entry_id");
    let copy_id = text(&copy["conflict"], "entry_id");
    assert!(text(&copy["conflict"], "of") == original_id);
    assert!(copy_id != original_id);
    let copy_name = text(&copy["conflict"], "name");
    assert!(
        copy_name.starts_with(&format!("{stem}.sync-conflict-")) && copy_name.ends_with(".txt")
    );
    for d in [&a, &b] {
        let listing = d.ok(&list);
        let count = rows(&listing, "entries")
            .iter()
            .filter(|v| text(v, "name").starts_with(&stem))
            .count();
        assert!(count == 2);
        let original_row = entry(&listing, &name);
        assert!(text(original_row, "id") == original_id);
        assert!(text(original_row, "content_id") == text(&x, "content_id"));
        let copy_row = entry(&listing, copy_name);
        assert!(text(copy_row, "id") == copy_id);
        assert!(text(copy_row, "content_id") == text(&y, "content_id"));
    }
}

#[test]
#[ignore = "needs two scratch devices and a fresh per-run move folder"]
fn stable_folder_id_survives_remote_move_and_rename() {
    let cfg = live();
    let reader = Device::new(&cfg, "EDITOR");
    let mover = Device::new(&cfg, "PEER");
    assert!(reader.client != mover.client);
    let folder = format!("Move-{}", cfg.run);
    let old_path = format!("{folder}/Data.bin");
    let before = reader.ok(&["ls", "--drive", &cfg.drive, &folder]);
    assert!(rows(&before, "entries").is_empty());
    let id = text(&before["folder"], "id");
    let put = mover.put(&old_path, b"cached contents");
    let cold = reader.get(&old_path, b"cached contents");
    assert!(cold["fetched"] == true);
    let initial = reader.ok(&["ls", "--entry", id]);
    assert!(text(entry(&initial, "Data.bin"), "path") == old_path);
    let new_name = format!("B/{}", unique("Renamed"));
    let moved = mover.ok(&[
        "move",
        "--entry",
        id,
        "--to-drive",
        &cfg.drive,
        "--to",
        &new_name,
    ]);
    assert!(moved["outcome"] == "accepted");
    let after = reader.ok(&["ls", "--entry", id]);
    let [child] = rows(&after, "entries") else {
        panic!("expected original child only")
    };
    assert!(after["folder"]["id"] == id);
    assert!(text(child, "id") == text(&put, "entry_id"));
    assert!(child["path"] == format!("{new_name}/Data.bin"));
    let cached = reader.get(&format!("{new_name}/DATA.BIN"), b"cached contents");
    assert!(cached["fetched"] == false);
    assert!(cached["grants"] == 0);
    assert!(cached["bytes_fetched"] == 0);
    let absent = reader.call(&["get", "--drive", &cfg.drive, &old_path], 2, false);
    assert!(absent["code"] == "not_found");
}
