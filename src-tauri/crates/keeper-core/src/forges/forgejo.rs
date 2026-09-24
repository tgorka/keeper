//! Forgejo's answers (AD-335). keeper's forge grant (`write:repository`)
//! cannot read `/user/repos`, so the list is `repos/search` by the person's
//! numeric id, which the repository scope alone guards; the id is the
//! UserInfo `sub`.

use serde::Deserialize;

use super::listing::{ForgeRepo, RawRepo};
use super::tokens::ForgeError;

/// Forgejo clamps `limit` to 50.
pub const PAGE_SIZE: u64 = 50;
/// Pages keeper reads per list.
pub const PAGE_CAP: u64 = 20;

/// Page `page` (from 1) of everything `uid` can reach, by name. Pages are
/// numbered against keeper's own `api_base`: the `Link` header points at the
/// forge's ROOT_URL, which may be another host.
pub fn search_url(api_base: &str, uid: &str, page: u64) -> String {
    let mut url = format!("{api_base}/repos/search?");
    url.push_str(
        &url::form_urlencoded::Serializer::new(String::new())
            .append_pair("uid", uid)
            .append_pair("private", "true")
            .append_pair("limit", &PAGE_SIZE.to_string())
            .append_pair("page", &page.to_string())
            .append_pair("sort", "alpha")
            .append_pair("order", "asc")
            .finish(),
    );
    url
}

/// The forge's UserInfo endpoint (no scope check).
pub fn userinfo_url(web_base: &str) -> String {
    format!("{web_base}/login/oauth/userinfo")
}

#[derive(Deserialize)]
struct Search {
    ok: bool,
    #[serde(default)]
    data: Vec<RawRepo>,
}

/// `{ok, data: [Repository]}`.
pub fn parse_search(body: &[u8]) -> Result<Vec<ForgeRepo>, ForgeError> {
    let search: Search = serde_json::from_slice(body).map_err(|_| {
        ForgeError::Refused("The forge sent a list keeper could not read.".to_owned())
    })?;
    if !search.ok {
        return Err(ForgeError::Refused(
            "The forge could not search your repositories.".to_owned(),
        ));
    }
    Ok(search
        .data
        .into_iter()
        .map(|repo| repo.into_repo(false))
        .collect())
}

#[derive(Deserialize)]
struct UserInfo {
    sub: Option<serde_json::Value>,
    preferred_username: Option<String>,
}

/// The `sub` (Forgejo's numeric user id, as a string or a number).
pub fn parse_userinfo_sub(body: &[u8]) -> Option<String> {
    let info: UserInfo = serde_json::from_slice(body).ok()?;
    match info.sub? {
        serde_json::Value::String(sub) if !sub.is_empty() => Some(sub),
        serde_json::Value::Number(sub) => Some(sub.to_string()),
        _ => None,
    }
}

/// The login UserInfo names, for the "you" group.
pub fn parse_userinfo_login(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<UserInfo>(body)
        .ok()?
        .preferred_username
        .filter(|login| !login.is_empty())
}

/// Pages to read for `total` repositories, and whether the cap cuts some off.
pub fn pages(total: u64) -> (u64, bool) {
    let needed = total.div_ceil(PAGE_SIZE);
    (needed.min(PAGE_CAP), needed > PAGE_CAP)
}

/// `X-Total-Count`.
pub fn total_count(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get("x-total-count")?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_search_answer_is_ok_and_data_or_a_refusal() {
        let body = br#"{"ok":true,"data":[
            {"name":"keeper-config","full_name":"keeper/keeper-config",
             "owner":{"id":3,"login":"keeper","username":"keeper"},
             "description":"","private":true,"fork":false,"archived":false,"mirror":true,
             "template":true,"default_branch":"main",
             "clone_url":"https://git.acme.dev/git/keeper/keeper-config.git",
             "html_url":"https://git.acme.dev/git/keeper/keeper-config",
             "updated_at":"2026-09-01T10:00:00+02:00","size":12,
             "permissions":{"admin":true,"push":true,"pull":true}}]}"#;
        let repos = parse_search(body).expect("search");
        assert_eq!(repos.len(), 1);
        let repo = &repos[0];
        assert_eq!(repo.owner, "keeper");
        assert!(repo.mirror && repo.template && repo.can_push && repo.private);
        assert_eq!(repo.description, None, "an empty description is none");
        assert_eq!(repo.updated_ms, Some(1_788_249_600_000));
        assert!(parse_search(br#"{"ok":false,"data":[]}"#).is_err());
        assert!(parse_search(br#"[]"#).is_err());
    }

    #[test]
    fn pages_follow_the_total_and_stop_at_twenty() {
        assert_eq!(pages(0), (0, false));
        assert_eq!(pages(1), (1, false));
        assert_eq!(pages(50), (1, false));
        assert_eq!(pages(51), (2, false));
        assert_eq!(pages(1000), (20, false));
        assert_eq!(pages(1001), (20, true));
    }

    #[test]
    fn the_userinfo_sub_is_the_uid_whether_string_or_number() {
        assert_eq!(
            parse_userinfo_sub(br#"{"sub":"7","preferred_username":"tgorka"}"#).as_deref(),
            Some("7")
        );
        assert_eq!(parse_userinfo_sub(br#"{"sub":7}"#).as_deref(), Some("7"));
        assert_eq!(parse_userinfo_sub(br#"{"sub":""}"#), None);
        assert_eq!(parse_userinfo_sub(br#"{"name":"x"}"#), None);
        assert_eq!(
            parse_userinfo_login(br#"{"sub":"7","preferred_username":"tgorka"}"#).as_deref(),
            Some("tgorka")
        );
    }

    #[test]
    fn the_search_url_pages_by_number_against_keeper_s_own_api_base() {
        assert_eq!(
            search_url("https://git.acme.dev/git/api/v1", "7", 3),
            "https://git.acme.dev/git/api/v1/repos/search?uid=7&private=true&limit=50&page=3&sort=alpha&order=asc"
        );
    }
}
