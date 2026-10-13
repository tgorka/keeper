//! Gradle's half of the licence firewall (Epic 98, story 98.3 #6; NFR-119, F15).
//!
//! `cargo deny` never sees the Android build's Gradle dependencies, and they
//! ship in the APK as surely as a crate ships in the binary. So this reads
//! every `group:artifact:version` declared in `gen/android`'s Gradle build
//! files and in the Android libraries of the Tauri crates `Cargo.lock` pins
//! (`tauri`'s `mobile/android`, each `tauri-plugin-*`'s `android/`), and
//! requires each to be a row of `docs/android.md` § *Dependencies and
//! licences* whose licence `src-tauri/deny.toml` allows. Test configurations
//! are not read: they neither ship nor run in keeper's build. A GPL or AGPL
//! licence, a licence the firewall does not list, a coordinate with no row,
//! or a row nothing declares any more is a failure.
//!
//! What only Gradle resolves — the libraries these coordinates pull in
//! transitively — is the second half: `the_resolved_android_classpath_is_licensed`
//! reads the report CI's `android` job has Gradle write for the APK's runtime
//! classpath and requires every group in it to be covered by a row of
//! § *What Gradle resolves*, whose licences are checked here on every run.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The repository root, from this crate's manifest (`src-tauri/crates/keeper-core`).
fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("gradle licence scan: cannot read {}: {e}", path.display()))
}

/// Is `text` a Maven coordinate with a version (`group:artifact:version`)?
fn is_coordinate(text: &str) -> bool {
    let parts: Vec<&str> = text.split(':').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
        })
}

/// Every coordinate in one Gradle file's string literals, outside line
/// comments and outside the test configurations (`testImplementation`,
/// `androidTestImplementation`, …), which neither ship in the APK nor run in
/// keeper's build — the Tauri libraries declare JUnit 4 (EPL-1.0) there.
fn coordinates(gradle: &str) -> Vec<String> {
    gradle
        .lines()
        .map(str::trim_start)
        .filter(|line| {
            !line.starts_with("//") && !line.starts_with("test") && !line.starts_with("androidTest")
        })
        .flat_map(|line| line.split('"').skip(1).step_by(2))
        .filter(|literal| is_coordinate(literal))
        .map(str::to_owned)
        .collect()
}

/// Gradle build files under `dir`, skipping build output and Gradle's caches.
fn gradle_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("gradle licence scan: cannot read {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("gradle licence scan: directory entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            if !matches!(name, "build" | ".gradle" | ".tauri") {
                gradle_files(&path, out);
            }
        } else if name.ends_with(".gradle.kts") || name.ends_with(".gradle") {
            out.push(path);
        }
    }
}

/// Is `name` one of the crates whose Android library ships in the APK?
fn is_tauri_crate(name: &str) -> bool {
    name == "tauri" || name.starts_with("tauri-plugin-")
}

/// The source directory of every registry package named `tauri` or
/// `tauri-plugin-*` in `cargo metadata`'s JSON. A path or git package is
/// keeper's own and has no Android library of interest.
fn tauri_crate_dirs(metadata: &str) -> Vec<(String, PathBuf)> {
    let metadata: serde_json::Value =
        serde_json::from_str(metadata).expect("gradle licence scan: cargo metadata is not JSON");
    let packages = metadata["packages"]
        .as_array()
        .expect("gradle licence scan: cargo metadata has no packages");
    packages
        .iter()
        .filter(|package| {
            package["source"]
                .as_str()
                .is_some_and(|source| source.starts_with("registry+"))
        })
        .filter_map(|package| {
            let name = package["name"]
                .as_str()
                .filter(|name| is_tauri_crate(name))?;
            let manifest = Path::new(package["manifest_path"].as_str()?);
            Some((name.to_owned(), manifest.parent()?.to_path_buf()))
        })
        .collect()
}

/// `cargo metadata` over the workspace, at `Cargo.lock`'s versions. Unlike a
/// `keeper-core`-only build, it downloads and unpacks every locked package —
/// tauri's and the plugins' sources included — so the scan never depends on
/// what this machine happened to compile before.
fn workspace_metadata(repo: &Path) -> String {
    let output = std::process::Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--manifest-path",
        ])
        .arg(repo.join("src-tauri/Cargo.toml"))
        .output()
        .expect("gradle licence scan: cannot run cargo metadata");
    assert!(
        output.status.success(),
        "gradle licence scan: cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("gradle licence scan: cargo metadata is not UTF-8")
}

/// The doc's section of declared coordinates.
const DECLARED: &str = "## Dependencies and licences";

/// The doc's section of the groups Gradle may resolve.
const RESOLVED: &str = "## What Gradle resolves";

/// First cell → licence for every row of the table under `heading`; a first
/// cell listed twice is reported in `problems`.
fn listed(doc: &str, heading: &str, problems: &mut Vec<String>) -> BTreeMap<String, String> {
    let mut rows = BTreeMap::new();
    let mut inside = false;
    for line in doc.lines() {
        if line.starts_with("## ") {
            inside = line.trim() == heading;
            continue;
        }
        if !inside || !line.starts_with("| `") {
            continue;
        }
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let (Some(coordinate), Some(licence)) = (cells.get(1), cells.get(2)) else {
            continue;
        };
        let coordinate = coordinate.trim_matches('`').to_owned();
        if rows
            .insert(coordinate.clone(), (*licence).to_owned())
            .is_some()
        {
            problems.push(format!("{coordinate} is listed twice"));
        }
    }
    rows
}

/// The `[licenses] allow` list of `deny.toml`.
fn allowed_licences(deny: &str) -> Vec<String> {
    let section = deny
        .split("\n[licenses]")
        .nth(1)
        .expect("gradle licence scan: deny.toml has no [licenses] section");
    let list = section
        .split_once("allow = [")
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(list, _)| list)
        .expect("gradle licence scan: deny.toml's [licenses] has no allow list");
    list.lines()
        .map(|line| line.split('#').next().unwrap_or_default())
        .flat_map(|line| line.split('"').skip(1).step_by(2))
        .map(str::to_owned)
        .collect()
}

/// Whether the SPDX expression `licence` passes: never anything GPL, and at
/// least one `OR` alternative whose every `AND` term the firewall allows.
fn licence_problem(licence: &str, allow: &[String]) -> Option<String> {
    if licence.contains("GPL") {
        return Some(format!("{licence} is a GPL-family licence"));
    }
    let passes = licence.split(" OR ").any(|alternative| {
        alternative
            .split(" AND ")
            .map(|term| term.trim().trim_matches(|c| c == '(' || c == ')').trim())
            .all(|term| allow.iter().any(|allowed| allowed == term))
    });
    (!passes).then(|| format!("{licence} is not a licence src-tauri/deny.toml allows"))
}

/// Everything wrong between what the build files declare (`coordinate` → the
/// files declaring it) and what the doc lists.
fn problems(
    declared: &BTreeMap<String, Vec<String>>,
    listed: &BTreeMap<String, String>,
    allow: &[String],
) -> Vec<String> {
    let mut found = Vec::new();
    for (coordinate, files) in declared {
        match listed.get(coordinate) {
            None => found.push(format!(
                "{coordinate} (declared in {}) has no row in docs/android.md § Dependencies and \
                 licences",
                files.join(", ")
            )),
            Some(licence) => {
                if let Some(problem) = licence_problem(licence, allow) {
                    found.push(format!("{coordinate}: {problem}"));
                }
            }
        }
    }
    for coordinate in listed.keys() {
        if !declared.contains_key(coordinate) {
            found.push(format!(
                "{coordinate} is listed in docs/android.md but no Gradle file declares it"
            ));
        }
    }
    found
}

/// What a `gradle :app:dependencies --configuration …` report resolved: every
/// `group:artifact` in its trees (constraints included — a constraint pins a
/// version of something that is in the tree), and every line Gradle marked
/// `FAILED`, whose subtree nobody can vouch for. Project lines
/// (`project :tauri-android`) are keeper's or Tauri's own modules, whose
/// external dependencies appear beneath them.
fn resolved(report: &str) -> (BTreeSet<String>, Vec<String>) {
    let mut artifacts = BTreeSet::new();
    let mut failed = Vec::new();
    for line in report.lines() {
        let Some((_, entry)) = line.split_once("--- ") else {
            continue;
        };
        if entry.starts_with("project ") {
            continue;
        }
        if entry.contains("FAILED") {
            failed.push(entry.trim().to_owned());
            continue;
        }
        let mut parts = entry
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .split(':');
        if let (Some(group), Some(artifact)) = (parts.next(), parts.next()) {
            if !group.is_empty() && !artifact.is_empty() {
                artifacts.insert(format!("{group}:{artifact}"));
            }
        }
    }
    (artifacts, failed)
}

/// Does the allow-list row `pattern` cover `group`? A row is one group, or a
/// group and every group beneath it when it ends in `.*` (`androidx.*`).
fn covers(pattern: &str, group: &str) -> bool {
    match pattern.strip_suffix(".*") {
        Some(prefix) => {
            group == prefix
                || group
                    .strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with('.'))
        }
        None => pattern == group,
    }
}

/// Everything wrong between what Gradle resolved and the allow-list of groups:
/// an artifact no row covers, a row whose licence the firewall refuses, a row
/// nothing resolved matches any more, an unresolved line, an empty report.
fn resolved_problems(
    report: &str,
    groups: &BTreeMap<String, String>,
    allow: &[String],
) -> Vec<String> {
    let (artifacts, failed) = resolved(report);
    let mut found: Vec<String> = failed
        .into_iter()
        .map(|line| format!("Gradle could not resolve {line}"))
        .collect();
    if artifacts.is_empty() {
        found.push("the report names no dependency — it must never pass vacuously".to_owned());
    }
    for artifact in &artifacts {
        let group = artifact.split(':').next().unwrap_or_default();
        if !groups.keys().any(|pattern| covers(pattern, group)) {
            found.push(format!(
                "{artifact}: its group has no row in docs/android.md § What Gradle resolves"
            ));
        }
    }
    for (pattern, licence) in groups {
        if let Some(problem) = licence_problem(licence, allow) {
            found.push(format!("{pattern}: {problem}"));
        }
        let used = artifacts
            .iter()
            .any(|artifact| covers(pattern, artifact.split(':').next().unwrap_or_default()));
        if !used {
            found.push(format!(
                "{pattern} is listed in docs/android.md but Gradle resolved nothing in it"
            ));
        }
    }
    found
}

#[test]
fn android_gradle_dependencies_are_licensed() {
    let repo = repo();
    let gen = repo.join("src-tauri/crates/keeper/gen/android");
    assert!(
        gen.is_dir(),
        "gradle licence scan: no Android project at {} — the scan must never pass vacuously",
        gen.display()
    );

    let mut files: Vec<(String, PathBuf)> = Vec::new();
    let mut own = Vec::new();
    gradle_files(&gen, &mut own);
    for file in own {
        let shown = file
            .strip_prefix(&repo)
            .unwrap_or(&file)
            .display()
            .to_string();
        files.push((shown, file));
    }
    let mut libraries = 0;
    for (name, dir) in tauri_crate_dirs(&workspace_metadata(&repo)) {
        for library in [
            "android/build.gradle.kts",
            "mobile/android/build.gradle.kts",
        ] {
            let path = dir.join(library);
            if path.is_file() {
                files.push((format!("{name}/{library}"), path));
                libraries += 1;
            }
        }
    }
    assert!(
        libraries > 0,
        "gradle licence scan: no Android library found in any Tauri crate — tauri's own \
         mobile/android must be among them"
    );

    let mut declared: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (shown, file) in &files {
        for coordinate in coordinates(&read(file)) {
            declared.entry(coordinate).or_default().push(shown.clone());
        }
    }
    assert!(
        declared
            .keys()
            .any(|c| c.starts_with("androidx.webkit:webkit:")),
        "gradle licence scan: the app's own dependencies were not read: {declared:?}"
    );

    let mut found = Vec::new();
    let listed = listed(&read(&repo.join("docs/android.md")), DECLARED, &mut found);
    let allow = allowed_licences(&read(&repo.join("src-tauri/deny.toml")));
    assert!(
        allow.iter().any(|licence| licence == "Apache-2.0"),
        "gradle licence scan: deny.toml's allow list did not parse: {allow:?}"
    );
    found.extend(problems(&declared, &listed, &allow));
    assert!(
        found.is_empty(),
        "Gradle dependencies outside the licence firewall:\n  {}",
        found.join("\n  ")
    );
}

/// The allow-list of groups and the firewall's licences, read from the repo;
/// a group listed twice is a problem in `found`.
fn resolved_groups(found: &mut Vec<String>) -> (BTreeMap<String, String>, Vec<String>) {
    let repo = repo();
    let groups = listed(&read(&repo.join("docs/android.md")), RESOLVED, found);
    assert!(
        groups.contains_key("androidx.*"),
        "gradle licence scan: § What Gradle resolves did not parse: {groups:?}"
    );
    (
        groups,
        allowed_licences(&read(&repo.join("src-tauri/deny.toml"))),
    )
}

/// On every run: each group the APK may carry is listed under a licence the
/// firewall allows, once.
#[test]
fn every_group_gradle_may_resolve_carries_an_allowed_licence() {
    let mut found = Vec::new();
    let (groups, allow) = resolved_groups(&mut found);
    for (pattern, licence) in &groups {
        if let Some(problem) = licence_problem(licence, &allow) {
            found.push(format!("{pattern}: {problem}"));
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n  "));
}

/// What Gradle actually resolves for the APK, transitive dependencies
/// included, against § *What Gradle resolves*. Needs the report only Gradle
/// can write, so it runs where the APK is built — CI's `android` job, which
/// writes `./gradlew -q :app:dependencies --configuration
/// universal{Debug,Release}RuntimeClasspath` to the file
/// `KEEPER_ANDROID_CLASSPATH` names and runs this with `--ignored`.
#[test]
#[ignore = "needs KEEPER_ANDROID_CLASSPATH, a Gradle dependencies report (CI's android job)"]
fn the_resolved_android_classpath_is_licensed() {
    let path = std::env::var_os("KEEPER_ANDROID_CLASSPATH").expect(
        "gradle licence scan: KEEPER_ANDROID_CLASSPATH must name a `gradle :app:dependencies` report",
    );
    let report = read(Path::new(&path));
    assert!(
        report.contains("androidx.webkit:webkit:"),
        "gradle licence scan: the report does not hold the app's runtime classpath"
    );
    let mut found = Vec::new();
    let (groups, allow) = resolved_groups(&mut found);
    found.extend(resolved_problems(&report, &groups, &allow));
    assert!(
        found.is_empty(),
        "Resolved Gradle dependencies outside the licence firewall:\n  {}",
        found.join("\n  ")
    );
}

#[cfg(test)]
mod the_scan {
    use super::*;

    fn allow() -> Vec<String> {
        ["Apache-2.0", "MIT", "BSD-3-Clause"]
            .map(str::to_owned)
            .to_vec()
    }

    fn declared(coordinates: &[&str]) -> BTreeMap<String, Vec<String>> {
        coordinates
            .iter()
            .map(|c| ((*c).to_owned(), vec!["app/build.gradle.kts".to_owned()]))
            .collect()
    }

    fn rows(rows: &[(&str, &str)]) -> BTreeMap<String, String> {
        rows.iter()
            .map(|(c, l)| ((*c).to_owned(), (*l).to_owned()))
            .collect()
    }

    #[test]
    fn reads_declared_coordinates_and_nothing_else() {
        let gradle = r#"
            dependencies {
                implementation("androidx.webkit:webkit:1.14.0")
                classpath("com.android.tools.build:gradle:8.11.0")
                // implementation("org.example:commented:1.0")
                testImplementation("junit:junit:4.13.2")
                androidTestImplementation("androidx.test.ext:junit:1.1.5")
                implementation(project(":tauri-android"))
                id("org.jetbrains.kotlin.android")
                manifestPlaceholders["usesCleartextTraffic"] = "false"
            }
        "#;
        assert_eq!(
            coordinates(gradle),
            [
                "androidx.webkit:webkit:1.14.0",
                "com.android.tools.build:gradle:8.11.0"
            ]
        );
    }

    #[test]
    fn an_unlisted_coordinate_fails_and_a_listed_one_passes() {
        let declared = declared(&["a.b:c:1.0", "d.e:f:2.0"]);
        let listed = rows(&[("a.b:c:1.0", "Apache-2.0")]);
        let found = problems(&declared, &listed, &allow());
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with("d.e:f:2.0"), "{found:?}");
    }

    #[test]
    fn a_new_version_needs_its_own_row() {
        let found = problems(
            &declared(&["a.b:c:1.1"]),
            &rows(&[("a.b:c:1.0", "Apache-2.0")]),
            &allow(),
        );
        assert_eq!(found.len(), 2, "unlisted 1.1 and stale 1.0: {found:?}");
    }

    #[test]
    fn gpl_and_unknown_licences_fail_even_beside_an_allowed_one() {
        for licence in [
            "GPL-3.0-only",
            "AGPL-3.0",
            "LGPL-2.1",
            "MIT OR GPL-2.0",
            "EPL-1.0",
            "",
        ] {
            let found = problems(
                &declared(&["a.b:c:1.0"]),
                &rows(&[("a.b:c:1.0", licence)]),
                &allow(),
            );
            assert_eq!(found.len(), 1, "{licence:?} must fail: {found:?}");
        }
    }

    #[test]
    fn an_or_with_one_allowed_side_and_an_and_of_allowed_terms_pass() {
        for licence in [
            "Apache-2.0",
            "EPL-2.0 OR Apache-2.0",
            "(MIT AND BSD-3-Clause)",
        ] {
            let found = problems(
                &declared(&["a.b:c:1.0"]),
                &rows(&[("a.b:c:1.0", licence)]),
                &allow(),
            );
            assert!(found.is_empty(), "{licence:?}: {found:?}");
        }
        let found = problems(
            &declared(&["a.b:c:1.0"]),
            &rows(&[("a.b:c:1.0", "MIT AND EPL-1.0")]),
            &allow(),
        );
        assert_eq!(found.len(), 1, "every AND term must be allowed: {found:?}");
    }

    #[test]
    fn only_the_licence_section_counts_and_a_twice_listed_row_is_reported() {
        let doc =
            "# x\n\n## Other\n\n| `x.y:z:1.0` | MIT | here |\n\n## Dependencies and licences\n\n\
                   | Coordinate | Licence | Declared by |\n|---|---|---|\n\
                   | `a.b:c:1.0` | Apache-2.0 | app |\n| `a.b:c:1.0` | MIT | app |\n\n## After\n\n\
                   | `p.q:r:1.0` | MIT | there |\n";
        let mut found = Vec::new();
        let rows = listed(doc, DECLARED, &mut found);
        assert_eq!(rows.keys().collect::<Vec<_>>(), ["a.b:c:1.0"]);
        assert_eq!(found, ["a.b:c:1.0 is listed twice"]);
    }

    #[test]
    fn deny_toml_comments_are_not_licences() {
        let deny = "[bans]\nx = 1\n[licenses]\nallow = [\n  \"MIT\", # \"GPL-3.0\"\n  \"Apache-2.0\",\n]\n";
        assert_eq!(allowed_licences(deny), ["MIT", "Apache-2.0"]);
    }

    #[test]
    fn the_metadata_names_tauri_and_its_plugins_from_the_registry_only() {
        let metadata = r#"{"packages":[
            {"name":"tauri","source":"registry+https://x","manifest_path":"/r/tauri-2.11.5/Cargo.toml"},
            {"name":"tauri-plugin-opener","source":"registry+https://x","manifest_path":"/r/tauri-plugin-opener-2.5.4/Cargo.toml"},
            {"name":"tauri-build","source":"registry+https://x","manifest_path":"/r/tauri-build-2.6.3/Cargo.toml"},
            {"name":"tauri-plugin-local","source":null,"manifest_path":"/w/tauri-plugin-local/Cargo.toml"}
        ]}"#;
        assert_eq!(
            tauri_crate_dirs(metadata),
            [
                ("tauri".to_owned(), PathBuf::from("/r/tauri-2.11.5")),
                (
                    "tauri-plugin-opener".to_owned(),
                    PathBuf::from("/r/tauri-plugin-opener-2.5.4")
                )
            ]
        );
    }

    /// An excerpt of a real `:app:dependencies` report's shape.
    const REPORT: &str = "
------------------------------------------------------------
Project ':app'
------------------------------------------------------------

universalDebugRuntimeClasspath - Runtime classpath of /universalDebug.
+--- org.jetbrains.kotlin:kotlin-stdlib:1.9.25 -> 2.0.21
|    \\--- org.jetbrains:annotations:13.0 -> 23.0.0
+--- androidx.webkit:webkit:1.14.0
|    +--- androidx.core:core:1.13.0 -> 1.15.0 (*)
|    \\--- androidx.lifecycle:lifecycle-runtime:{strictly 2.6.1} -> 2.6.1 (c)
\\--- project :tauri-android
     \\--- com.fasterxml.jackson.core:jackson-databind:2.15.3

(c) - A dependency constraint, not a dependency. The dependency affected by the constraint occurs elsewhere in the tree.
(*) - Indicates repeated occurrences of a transitive dependency subtree.
";

    fn groups(rows: &[&str]) -> BTreeMap<String, String> {
        rows.iter()
            .map(|g| ((*g).to_owned(), "Apache-2.0".to_owned()))
            .collect()
    }

    const ALL: [&str; 4] = [
        "androidx.*",
        "org.jetbrains",
        "org.jetbrains.kotlin",
        "com.fasterxml.jackson.core",
    ];

    #[test]
    fn a_report_yields_every_artifact_in_its_trees_and_nothing_else() {
        let (artifacts, failed) = resolved(REPORT);
        assert_eq!(
            artifacts.into_iter().collect::<Vec<_>>(),
            [
                "androidx.core:core",
                "androidx.lifecycle:lifecycle-runtime",
                "androidx.webkit:webkit",
                "com.fasterxml.jackson.core:jackson-databind",
                "org.jetbrains.kotlin:kotlin-stdlib",
                "org.jetbrains:annotations",
            ]
        );
        assert!(failed.is_empty());
        assert!(resolved_problems(REPORT, &groups(&ALL), &allow()).is_empty());
    }

    #[test]
    fn a_group_no_row_covers_fails_and_a_wildcard_covers_only_beneath_itself() {
        let found = resolved_problems(
            REPORT,
            &groups(&[
                "androidx.*",
                "org.jetbrains.kotlin",
                "com.fasterxml.jackson.core",
            ]),
            &allow(),
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].starts_with("org.jetbrains:annotations"),
            "{found:?}"
        );

        assert!(covers("androidx.*", "androidx"));
        assert!(covers("androidx.*", "androidx.core"));
        assert!(!covers("androidx.*", "androidxevil.core"));
        assert!(
            !covers("org.jetbrains", "org.jetbrains.kotlin"),
            "an exact row is exact"
        );
    }

    #[test]
    fn a_refused_licence_a_stale_row_a_failed_line_and_an_empty_report_fail() {
        let mut gpl = groups(&ALL);
        gpl.insert("org.jetbrains".to_owned(), "GPL-2.0-only".to_owned());
        assert_eq!(resolved_problems(REPORT, &gpl, &allow()).len(), 1);

        let mut stale = ALL.to_vec();
        stale.push("org.example");
        let found = resolved_problems(REPORT, &groups(&stale), &allow());
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with("org.example"), "{found:?}");

        let failed = format!("{REPORT}+--- org.jetbrains:missing:1.0 FAILED\n");
        let found = resolved_problems(&failed, &groups(&ALL), &allow());
        assert_eq!(
            found.len(),
            1,
            "an unresolved line is never covered by its group: {found:?}"
        );

        let empty = "universalDebugRuntimeClasspath - Runtime classpath\nNo dependencies\n";
        assert!(!resolved_problems(empty, &groups(&[]), &allow()).is_empty());
    }
}
