use serde_json::Value;

const CORPUS: &str = include_str!("../../../../syncular/files.vectors.json");

fn corpus() -> Value {
    serde_json::from_str(CORPUS).expect("valid files.vectors.json")
}

#[test]
fn names_match_files_vectors() {
    let root = corpus();
    let items = root["names"]
        .as_array()
        .expect("files.vectors.json names array");
    let mut failures = Vec::new();

    assert_eq!(keeper_paths::MAX_NAME_BYTES, 255);

    for item in items {
        let id = item["id"].as_str().expect("names[].id");
        let name = item["name"].as_str().expect("names[].name");
        let admitted = item["admitted"].as_bool().expect("names[].admitted");

        let got_admitted = name.len() <= keeper_paths::MAX_NAME_BYTES
            && !name.contains('\0')
            && matches!(keeper_paths::plain_segments(name), Ok(segs) if segs.len() == 1);

        if got_admitted != admitted {
            failures.push(format!(
                "{id}: admitted({name:?}) expected {admitted}, got {got_admitted}"
            ));
            continue;
        }

        if let Some(expected_fold) = item
            .get("fold")
            .and_then(Value::as_str)
            .filter(|_| admitted)
        {
            let got_fold = keeper_paths::names::name_fold(name);
            if got_fold != expected_fold {
                failures.push(format!(
                    "{id}: name_fold({name:?}) expected {expected_fold:?}, got {got_fold:?}"
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "names: {} failure(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn folds_match_files_vectors() {
    let root = corpus();
    let items = root["folds"]
        .as_array()
        .expect("files.vectors.json folds array");
    let mut failures = Vec::new();

    for item in items {
        let id = item["id"].as_str().expect("folds[].id");
        let a = item["a"].as_str().expect("folds[].a");
        let b = item["b"].as_str().expect("folds[].b");
        let same = item["same"].as_bool().expect("folds[].same");

        let by_same = keeper_paths::names::same_entry_folded(a, b);
        let by_fold = keeper_paths::names::name_fold(a) == keeper_paths::names::name_fold(b);

        if by_same != same {
            failures.push(format!(
                "{id}: same_entry_folded({a:?}, {b:?}) expected {same}, got {by_same}"
            ));
        }
        if by_fold != same {
            failures.push(format!(
                "{id}: name_fold({a:?}) == name_fold({b:?}) expected {same}, got {by_fold}"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "folds: {} failure(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
