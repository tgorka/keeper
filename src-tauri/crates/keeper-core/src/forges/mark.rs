//! Each repository against the drives already here and on other devices,
//! and the owner groups the sheet shows (AD-335). Pure.

use super::listing::{ForgeRepo, Listing};
use super::vm::{ForgeNoticeVm, ForgeOwnerVm, ForgeRepoVm, ForgeReposVm};
use crate::org_account::manifest::DriveRecord;
use crate::org_account::settings_sync::{normalize_remote, LocalDrive};

/// `(added_as, elsewhere)` for one clone URL: this device's drive names on
/// it, and the other devices' slugs that sync it. Remotes compare in
/// [`normalize_remote`]'s spelling, so `.git`, case and a trailing slash do
/// not matter.
pub fn marks(
    clone_url: &str,
    local: &[LocalDrive],
    records: &[DriveRecord],
    this_device: &str,
) -> (Vec<String>, Vec<String>) {
    let wanted = normalize_remote(clone_url);
    let added_as = local
        .iter()
        .filter(|drive| normalize_remote(&drive.remote_url) == wanted)
        .map(|drive| drive.name.clone())
        .collect();
    let mut elsewhere: Vec<String> = records
        .iter()
        .filter(|record| normalize_remote(&record.remote_url) == wanted)
        .flat_map(|record| &record.devices)
        .filter(|device| device.as_str() != this_device)
        .cloned()
        .collect();
    elsewhere.sort();
    elsewhere.dedup();
    (added_as, elsewhere)
}

/// The sheet's list: repositories grouped by owner — you first, then the
/// rest A→Z — and by name within a group, each marked.
pub fn repos_vm(
    source_id: &str,
    listing: &Listing,
    local: &[LocalDrive],
    records: &[DriveRecord],
    this_device: &str,
) -> ForgeReposVm {
    let mut repos: Vec<&ForgeRepo> = listing.repos.iter().collect();
    repos.sort_by_cached_key(|repo| listing.group_key(repo));
    let mut owners: Vec<ForgeOwnerVm> = Vec::new();
    for repo in &repos {
        match owners.last_mut() {
            Some(last) if last.login.eq_ignore_ascii_case(&repo.owner) => last.count += 1,
            _ => owners.push(ForgeOwnerVm {
                login: repo.owner.clone(),
                is_you: listing.is_you(&repo.owner),
                count: 1,
            }),
        }
    }
    ForgeReposVm {
        source_id: source_id.to_owned(),
        repos: repos
            .into_iter()
            .map(|repo| {
                let (added_as, elsewhere) = marks(&repo.clone_url, local, records, this_device);
                ForgeRepoVm {
                    full_name: repo.full_name.clone(),
                    owner: repo.owner.clone(),
                    name: repo.name.clone(),
                    description: repo.description.clone(),
                    private: repo.private,
                    fork: repo.fork,
                    archived: repo.archived,
                    template: repo.template,
                    mirror: repo.mirror,
                    default_branch: repo.default_branch.clone(),
                    clone_url: repo.clone_url.clone(),
                    web_url: repo.web_url.clone(),
                    updated_ms: repo.updated_ms,
                    size_kb: repo.size_kb,
                    can_push: repo.can_push,
                    pull_only: repo.pull_only(),
                    pull_only_sentence: repo.pull_only_sentence().map(str::to_owned),
                    added_as,
                    elsewhere,
                }
            })
            .collect(),
        owners,
        notices: listing
            .notices
            .iter()
            .map(|notice| ForgeNoticeVm {
                sentence: notice.sentence.clone(),
                link: notice.link.clone(),
            })
            .collect(),
        truncated: listing.truncated,
        fetched_ms: Some(listing.fetched_ms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(owner: &str, name: &str) -> ForgeRepo {
        ForgeRepo {
            full_name: format!("{owner}/{name}"),
            owner: owner.to_owned(),
            name: name.to_owned(),
            description: None,
            private: false,
            fork: false,
            archived: false,
            template: false,
            mirror: false,
            default_branch: "main".to_owned(),
            clone_url: format!("https://github.com/{owner}/{name}.git"),
            web_url: format!("https://github.com/{owner}/{name}"),
            updated_ms: None,
            size_kb: None,
            can_push: true,
        }
    }

    fn local(name: &str, remote: &str, path: &str) -> LocalDrive {
        LocalDrive {
            profile_id: format!("01{name}"),
            remote_url: remote.to_owned(),
            branch: "main".to_owned(),
            name: name.to_owned(),
            local_path: path.to_owned(),
        }
    }

    fn record(remote: &str, devices: &[&str]) -> DriveRecord {
        DriveRecord {
            remote_url: remote.to_owned(),
            devices: devices.iter().map(|d| (*d).to_owned()).collect(),
            ..DriveRecord::default()
        }
    }

    #[test]
    fn marks_match_every_spelling_of_the_same_remote() {
        let drives = [
            local(
                "Notes",
                "https://GitHub.com/tgorka/notes/",
                "/Users/tg/notes",
            ),
            local(
                "Notes light",
                "https://oauth2:tok@github.com/tgorka/notes",
                "/Users/tg/light",
            ),
            local(
                "Other",
                "https://github.com/tgorka/other.git",
                "/Users/tg/other",
            ),
        ];
        let records = [
            record(
                "https://github.com/tgorka/notes",
                &["mac", "phone", "studio"],
            ),
            record("HTTPS://GITHUB.COM/tgorka/notes.git", &["studio", "ipad"]),
        ];
        let (added_as, elsewhere) = marks(
            "https://github.com/tgorka/notes.git",
            &drives,
            &records,
            "mac",
        );
        assert_eq!(added_as, ["Notes", "Notes light"]);
        assert_eq!(elsewhere, ["ipad", "phone", "studio"]);
        let (added_as, elsewhere) =
            marks("https://github.com/acme/site.git", &drives, &records, "mac");
        assert!(added_as.is_empty() && elsewhere.is_empty());
    }

    #[test]
    fn you_come_first_then_owners_a_to_z_each_with_its_count() {
        let listing = Listing {
            login: Some("tgorka".to_owned()),
            you: vec!["tgorka".to_owned()],
            repos: vec![
                repo("zeta", "b"),
                repo("acme", "site"),
                repo("TGorka", "notes"),
                repo("acme", "api"),
                repo("tgorka", "keeper"),
            ],
            notices: Vec::new(),
            truncated: false,
            fetched_ms: 1,
        };
        let vm = repos_vm("github", &listing, &[], &[], "mac");
        let names: Vec<&str> = vm.repos.iter().map(|r| r.full_name.as_str()).collect();
        assert_eq!(
            names,
            [
                "tgorka/keeper",
                "TGorka/notes",
                "acme/api",
                "acme/site",
                "zeta/b"
            ]
        );
        let owners: Vec<(&str, bool, u32)> = vm
            .owners
            .iter()
            .map(|o| (o.login.as_str(), o.is_you, o.count))
            .collect();
        assert_eq!(
            owners,
            [("tgorka", true, 2), ("acme", false, 2), ("zeta", false, 1)]
        );
    }
}
