//! GitHub's REST answers (AD-335): the repository list, its pages, and what
//! a refusal means for the listing.

use reqwest::header::HeaderMap;
use serde::Deserialize;

use super::listing::{ForgeRepo, RawRepo};
use super::tokens::ForgeError;

/// Pages keeper follows per list (100 repositories each).
pub const PAGE_CAP: usize = 10;

/// `GET` with the media type and API version keeper was written against, and
/// a `User-Agent`: api.github.com refuses a request without one with a plain
/// text 403 ("Request forbidden by administrative rules"), which reads as
/// "GitHub refused the list" and hides every repository.
pub fn get(http: &reqwest::Client, url: &str) -> reqwest::RequestBuilder {
    http.get(url)
        .header(reqwest::header::USER_AGENT, crate::bots::http::USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
}

/// The person's own, collaborator and organization repositories. Never
/// `type`: GitHub refuses it next to `affiliation`.
pub fn repos_url(api_base: &str) -> String {
    format!(
        "{api_base}/user/repos?affiliation=owner,collaborator,organization_member&visibility=all&per_page=100&sort=full_name"
    )
}

/// What an installation token reaches (the broker path).
pub fn installation_repos_url(api_base: &str) -> String {
    format!("{api_base}/installation/repositories?per_page=100")
}

/// A page of `/user/repos`.
pub fn parse_repos(body: &[u8]) -> Result<Vec<ForgeRepo>, ForgeError> {
    let raw: Vec<RawRepo> = serde_json::from_slice(body).map_err(|_| unreadable())?;
    Ok(raw.into_iter().map(|repo| repo.into_repo(false)).collect())
}

fn unreadable() -> ForgeError {
    ForgeError::Refused("GitHub sent a list keeper could not read.".to_owned())
}

/// A page of `/installation/repositories`.
#[derive(Debug, Clone, PartialEq)]
pub struct InstallationRepos {
    pub total_count: u64,
    pub repos: Vec<ForgeRepo>,
    /// Owners whose `type` is `User`: the "you" group of a broker listing.
    pub user_owners: Vec<String>,
}

#[derive(Deserialize)]
struct RawInstallation {
    total_count: u64,
    repositories: Vec<RawRepo>,
}

pub fn parse_installation_repos(body: &[u8]) -> Result<InstallationRepos, ForgeError> {
    let raw: RawInstallation = serde_json::from_slice(body).map_err(|_| unreadable())?;
    let mut user_owners: Vec<String> = Vec::new();
    let mut repos = Vec::with_capacity(raw.repositories.len());
    for repo in raw.repositories {
        if repo.owner.kind.as_deref() == Some("User") && !user_owners.contains(&repo.owner.login) {
            user_owners.push(repo.owner.login.clone());
        }
        // An installation token's `permissions` describe the listing token,
        // not the person; the broker's grants decide pushing (`listing`).
        repos.push(repo.into_installation_repo());
    }
    Ok(InstallationRepos {
        total_count: raw.total_count,
        repos,
        user_owners,
    })
}

/// The `rel="next"` URL of a `Link` header, if any.
pub fn next_link(link: &str) -> Option<String> {
    link.split(',').find_map(|part| {
        let (target, params) = part.split_once(';')?;
        let is_next = params.split(';').any(|param| {
            let param = param.trim();
            param == "rel=\"next\"" || param == "rel=next"
        });
        let target = target.trim().strip_prefix('<')?.strip_suffix('>')?;
        is_next.then(|| target.to_owned())
    })
}

/// What a refused page means for the listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GithubProblem {
    /// An organization has not approved keeper's OAuth App.
    Restricted,
    /// SAML single sign-on: the URL that authorizes keeper, when GitHub gave one.
    Sso(Option<String>),
    Error(ForgeError),
}

pub fn classify_error(status: u16, headers: &HeaderMap, body: &[u8]) -> GithubProblem {
    let message = serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(str::to_owned))
        .unwrap_or_default();
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    match status {
        401 => GithubProblem::Error(ForgeError::NeedsConnect),
        403 if message.contains("OAuth App access restrictions") => GithubProblem::Restricted,
        403 | 404 if header("x-github-sso").is_some() => {
            GithubProblem::Sso(header("x-github-sso").and_then(sso_url))
        }
        429 => GithubProblem::Error(rate_limited()),
        403 if header("x-ratelimit-remaining") == Some("0") => GithubProblem::Error(rate_limited()),
        500..=599 => GithubProblem::Error(ForgeError::Unreachable(
            "GitHub isn't answering right now.".to_owned(),
        )),
        _ if message.is_empty() => GithubProblem::Error(ForgeError::Refused(format!(
            "GitHub refused the list (HTTP {status})."
        ))),
        _ => GithubProblem::Error(ForgeError::Refused(format!(
            "GitHub refused the list: {message}"
        ))),
    }
}

fn rate_limited() -> ForgeError {
    ForgeError::Unreachable("GitHub is limiting requests; try again in a few minutes.".to_owned())
}

fn sso_url(value: &str) -> Option<String> {
    value
        .split(';')
        .find_map(|part| part.trim().strip_prefix("url="))
        .map(str::to_owned)
}

/// Whether a successful page left out SSO-protected organizations
/// (`X-GitHub-SSO: partial-results; organizations=…`).
pub fn sso_partial(headers: &HeaderMap) -> bool {
    headers
        .get("x-github-sso")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim_start().starts_with("partial-results"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    const PAGE: &str = r#"[
        {"name":"keeper","full_name":"tgorka/keeper","owner":{"login":"tgorka","type":"User"},
         "description":"A client","private":true,"fork":false,"archived":false,"is_template":false,
         "mirror_url":null,"default_branch":"main","clone_url":"https://github.com/tgorka/keeper.git",
         "html_url":"https://github.com/tgorka/keeper","updated_at":"2026-09-01T10:00:00Z",
         "size":2048,"permissions":{"admin":true,"push":true,"pull":true}},
        {"name":"mirror","full_name":"acme/mirror","owner":{"login":"acme","type":"Organization"},
         "description":null,"private":false,"fork":true,"archived":true,"is_template":true,
         "mirror_url":"https://example.org/x.git","default_branch":"trunk",
         "clone_url":"https://github.com/acme/mirror.git","html_url":"https://github.com/acme/mirror",
         "updated_at":null,"permissions":{"push":false}}
    ]"#;

    #[test]
    fn a_page_of_repositories_parses_every_field_keeper_shows() {
        let repos = parse_repos(PAGE.as_bytes()).expect("page");
        let keeper = &repos[0];
        assert_eq!(
            (
                keeper.full_name.as_str(),
                keeper.owner.as_str(),
                keeper.name.as_str()
            ),
            ("tgorka/keeper", "tgorka", "keeper")
        );
        assert_eq!(keeper.description.as_deref(), Some("A client"));
        assert!(keeper.private && keeper.can_push && !keeper.fork && !keeper.mirror);
        assert_eq!(keeper.default_branch, "main");
        assert_eq!(keeper.clone_url, "https://github.com/tgorka/keeper.git");
        assert_eq!(keeper.web_url, "https://github.com/tgorka/keeper");
        assert_eq!(keeper.updated_ms, Some(1_788_256_800_000));
        assert_eq!(keeper.size_kb, Some(2048));
        let mirror = &repos[1];
        assert!(mirror.fork && mirror.archived && mirror.template && mirror.mirror);
        assert!(!mirror.can_push);
        assert_eq!((mirror.updated_ms, mirror.size_kb), (None, None));
        assert!(parse_repos(b"{\"message\":\"x\"}").is_err());
    }

    #[test]
    fn the_next_page_comes_only_from_link_rel_next() {
        let both = r#"<https://api.github.com/user/repos?page=2>; rel="next", <https://api.github.com/user/repos?page=5>; rel="last""#;
        assert_eq!(
            next_link(both).as_deref(),
            Some("https://api.github.com/user/repos?page=2")
        );
        let last_page = r#"<https://api.github.com/user/repos?page=1>; rel="prev", <https://api.github.com/user/repos?page=1>; rel="first""#;
        assert_eq!(next_link(last_page), None);
        assert_eq!(next_link(""), None);
    }

    #[test]
    fn installation_pages_carry_the_total_and_which_owners_are_people() {
        let body = format!(r#"{{"total_count":2,"repositories":{PAGE}}}"#);
        let page = parse_installation_repos(body.as_bytes()).expect("page");
        assert_eq!(page.total_count, 2);
        assert_eq!(page.repos.len(), 2);
        assert_eq!(page.user_owners, ["tgorka"]);
        assert!(parse_installation_repos(PAGE.as_bytes()).is_err());
    }

    /// GitHub answers `/installation/repositories` with `permissions` taken
    /// from the token that asked. keeper lists with `metadata: read`, so every
    /// row says `push: false`; reading that as the person's rights made every
    /// broker repository download-only on hesperia (2026-09-24).
    #[test]
    fn an_installation_list_does_not_read_the_listing_token_s_rights_as_the_person_s() {
        let body = br#"{"total_count":1,"repositories":[{
            "name":"tokenizer","full_name":"Neuraffica/tokenizer",
            "owner":{"login":"Neuraffica","type":"Organization"},
            "private":true,"default_branch":"main",
            "clone_url":"https://github.com/Neuraffica/tokenizer.git",
            "html_url":"https://github.com/Neuraffica/tokenizer",
            "permissions":{"admin":false,"maintain":false,"push":false,"triage":false,"pull":true}}]}"#;
        let page = parse_installation_repos(body).expect("page");
        assert!(page.repos[0].can_push);
    }

    #[test]
    fn restriction_and_sso_refusals_become_notices_not_errors() {
        let none = HeaderMap::new();
        let restricted = br#"{"message":"Although you appear to have the correct authorization credentials, the `acme` organization has enabled OAuth App access restrictions, meaning that data access to third-parties is limited."}"#;
        assert_eq!(
            classify_error(403, &none, restricted),
            GithubProblem::Restricted
        );
        let mut sso = HeaderMap::new();
        sso.insert(
            "x-github-sso",
            HeaderValue::from_static(
                "required; url=https://github.com/orgs/acme/sso?authorization_request=abc",
            ),
        );
        assert_eq!(
            classify_error(
                403,
                &sso,
                br#"{"message":"Resource protected by organization SAML enforcement."}"#
            ),
            GithubProblem::Sso(Some(
                "https://github.com/orgs/acme/sso?authorization_request=abc".to_owned()
            ))
        );
        assert_eq!(
            classify_error(401, &none, b"{}"),
            GithubProblem::Error(ForgeError::NeedsConnect)
        );
        let mut limited = HeaderMap::new();
        limited.insert("x-ratelimit-remaining", HeaderValue::from_static("0"));
        assert!(matches!(
            classify_error(403, &limited, br#"{"message":"API rate limit exceeded"}"#),
            GithubProblem::Error(ForgeError::Unreachable(_))
        ));
        assert!(matches!(
            classify_error(403, &none, br#"{"message":"Forbidden"}"#),
            GithubProblem::Error(ForgeError::Refused(_))
        ));

        let mut partial = HeaderMap::new();
        partial.insert(
            "x-github-sso",
            HeaderValue::from_static("partial-results; organizations=21955855,20582480"),
        );
        assert!(sso_partial(&partial));
        assert!(!sso_partial(&sso) && !sso_partial(&none));
    }
}
