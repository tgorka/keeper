//! The listing (AD-335): every repository a source reaches, with the
//! notices that explain what is missing. Kept in memory per source and per
//! signed-in or connected identity for the process; `refresh` fetches
//! again. Nothing is written to disk.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, PoisonError};

use serde::Deserialize;

use super::broker::{self, BrokerFailure};
use super::github::{self, GithubProblem};
use super::source::{ForgeSource, TokenVia};
use super::tokens::{self, ForgeError};
use super::{body, device_flow, forgejo, now_ms, send};
use crate::org_account::descriptor::AccountDescriptor;
use crate::org_account::session;
use crate::platform::Platform;

/// The most repositories one listing holds.
pub const REPO_CAP: usize = 1000;
/// The most broker owners one listing asks about.
pub const OWNER_CAP: usize = 20;
/// The most requests one broker listing sends: `whoami`, a token per owner
/// and every page.
pub const REQUEST_CAP: usize = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeRepo {
    pub full_name: String,
    pub owner: String,
    pub name: String,
    pub description: Option<String>,
    pub private: bool,
    pub fork: bool,
    pub archived: bool,
    pub template: bool,
    pub mirror: bool,
    pub default_branch: String,
    pub clone_url: String,
    pub web_url: String,
    pub updated_ms: Option<i64>,
    pub size_kb: Option<u64>,
    pub can_push: bool,
}

impl ForgeRepo {
    /// A drive of this repository only downloads: keeper cannot push to it,
    /// or nobody should (archived, a mirror).
    pub fn pull_only(&self) -> bool {
        !self.can_push || self.archived || self.mirror
    }

    /// Why [`Self::pull_only`], as the row says it.
    pub fn pull_only_sentence(&self) -> Option<&'static str> {
        if self.archived {
            Some("This repository is archived, so keeper only downloads it.")
        } else if self.mirror {
            Some("This repository is a mirror, so keeper only downloads it.")
        } else if !self.can_push {
            Some("You can only read this repository, so keeper only downloads it.")
        } else {
            None
        }
    }
}

/// GitHub's and Forgejo's `Repository`, which agree on everything keeper
/// reads but the template and mirror spellings.
#[derive(Deserialize)]
pub(crate) struct RawRepo {
    name: String,
    full_name: String,
    pub(crate) owner: RawOwner,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    private: bool,
    #[serde(default)]
    fork: bool,
    #[serde(default)]
    archived: bool,
    #[serde(default, alias = "is_template")]
    template: bool,
    #[serde(default)]
    mirror: bool,
    #[serde(default)]
    mirror_url: Option<String>,
    #[serde(default)]
    default_branch: Option<String>,
    clone_url: String,
    html_url: String,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    permissions: Option<RawPermissions>,
}

#[derive(Deserialize)]
pub(crate) struct RawOwner {
    pub(crate) login: String,
    #[serde(rename = "type", default)]
    pub(crate) kind: Option<String>,
}

#[derive(Deserialize)]
struct RawPermissions {
    #[serde(default)]
    push: bool,
}

impl RawRepo {
    /// `can_push` is the answer's `permissions.push`, or `push_unsaid` when
    /// the answer has no `permissions` (an installation's list).
    pub(crate) fn into_repo(self, push_unsaid: bool) -> ForgeRepo {
        ForgeRepo {
            full_name: self.full_name,
            owner: self.owner.login,
            name: self.name,
            description: self.description.filter(|d| !d.trim().is_empty()),
            private: self.private,
            fork: self.fork,
            archived: self.archived,
            template: self.template,
            mirror: self.mirror || self.mirror_url.is_some_and(|url| !url.is_empty()),
            default_branch: self
                .default_branch
                .filter(|b| !b.is_empty())
                .unwrap_or_else(|| "main".to_owned()),
            clone_url: self.clone_url,
            web_url: self.html_url,
            updated_ms: self
                .updated_at
                .as_deref()
                .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                .map(|at| at.timestamp_millis()),
            size_kb: self.size.filter(|size| *size > 0),
            can_push: self.permissions.map_or(push_unsaid, |p| p.push),
        }
    }
}

/// One sentence above the list, with the link that fixes it when there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListNotice {
    pub sentence: String,
    pub link: Option<String>,
}

impl ListNotice {
    fn new(sentence: impl Into<String>, link: Option<String>) -> Self {
        ListNotice {
            sentence: sentence.into(),
            link,
        }
    }

    pub fn restricted(source: &ForgeSource) -> Self {
        ListNotice::new(
            "Some organizations haven't approved keeper, so their private repositories are hidden.",
            source.apps_url(),
        )
    }

    pub fn sso(link: Option<String>) -> Self {
        ListNotice::new(
            "Repositories of organizations that use single sign-on are hidden until you authorize keeper for them.",
            link,
        )
    }

    /// `listed` repositories were kept of more.
    pub fn truncated(listed: usize) -> Self {
        let listed = if listed >= 1000 {
            format!("{},{:03}", listed / 1000, listed % 1000)
        } else {
            listed.to_string()
        };
        ListNotice::new(
            format!("Only the first {listed} are listed; search looks only through those."),
            None,
        )
    }

    /// The fetch failed with `sentence` (which names the host that did not
    /// answer); the list shown is the one from `cached_ms`.
    pub fn offline(sentence: &str, cached_ms: Option<i64>) -> Self {
        let at = cached_ms
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|at| {
                format!(
                    " Showing the list from {}.",
                    at.with_timezone(&chrono::Local).format("%H:%M")
                )
            })
            .unwrap_or_default();
        ListNotice::new(format!("{sentence}{at}"), None)
    }

    fn owners_capped(broker_host: &str) -> Self {
        ListNotice::new(
            format!(
                "{broker_host} gives you more owners than keeper lists at once; only the first {OWNER_CAP} are listed."
            ),
            None,
        )
    }

    fn token_refused(owner: &str, broker_host: &str) -> Self {
        ListNotice::new(
            format!(
                "GitHub didn't accept the token {broker_host} gave keeper for {owner}; refresh to try again."
            ),
            None,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    /// Who the source says is connected, when it says.
    pub login: Option<String>,
    /// Owners grouped as "you".
    pub you: Vec<String>,
    pub repos: Vec<ForgeRepo>,
    pub notices: Vec<ListNotice>,
    pub truncated: bool,
    pub fetched_ms: i64,
}

impl Listing {
    fn new(login: Option<String>) -> Self {
        Listing {
            you: login.iter().cloned().collect(),
            login,
            repos: Vec::new(),
            notices: Vec::new(),
            truncated: false,
            fetched_ms: now_ms(),
        }
    }

    fn notice(&mut self, notice: ListNotice) {
        if !self.notices.contains(&notice) {
            self.notices.push(notice);
        }
    }

    pub(crate) fn is_you(&self, owner: &str) -> bool {
        self.you.iter().any(|you| you.eq_ignore_ascii_case(owner))
    }

    /// Where `repo` sits in the sheet: you first, then the other owners
    /// A→Z, by name within an owner.
    pub(crate) fn group_key(&self, repo: &ForgeRepo) -> (bool, String, String) {
        (
            !self.is_you(&repo.owner),
            repo.owner.to_lowercase(),
            repo.name.to_lowercase(),
        )
    }

    /// Grouped as the sheet shows it, then cut at [`REPO_CAP`]: what is cut
    /// is the end of the list the person sees, never their own repositories
    /// in favour of an organization's.
    fn finish(mut self) -> Self {
        let mut repos = std::mem::take(&mut self.repos);
        repos.sort_by_cached_key(|repo| self.group_key(repo));
        self.repos = repos;
        if self.repos.len() > REPO_CAP {
            self.repos.truncate(REPO_CAP);
            self.truncated = true;
        }
        if self.truncated {
            self.notice(ListNotice::truncated(self.repos.len()));
        }
        self
    }
}

/// Listings by (source id, whose they are).
type CacheKey = (String, String);

static CACHE: LazyLock<Mutex<HashMap<CacheKey, Listing>>> = LazyLock::new(Default::default);

/// Drop `source_id`'s listings: a connect or a disconnect changes them.
pub fn forget(source_id: &str) {
    CACHE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|(id, _), _| id != source_id);
}

/// Drop every listing: someone signed in or out, or the account changed.
pub fn forget_all() {
    CACHE.lock().unwrap_or_else(PoisonError::into_inner).clear();
}

/// Whose `source`'s listing would be: its device-flow connection's client,
/// or the signed-in person of the account. Without either, nothing is
/// fetched and nothing cached is served.
fn owner_of_listing(
    platform: &dyn Platform,
    source: &ForgeSource,
    account: Option<&AccountDescriptor>,
) -> Result<String, ForgeError> {
    if source.via == TokenVia::DeviceFlow {
        return tokens::load_session(platform, source)?
            .map(|stored| format!("device:{}", stored.client_id))
            .ok_or(ForgeError::NeedsConnect);
    }
    let sign_in =
        || ForgeError::NeedsSignIn("Sign in to your account to see its repositories.".to_owned());
    let d = account.ok_or_else(sign_in)?;
    let identity = session::identity(platform, d)?.ok_or_else(sign_in)?;
    Ok(format!("account:{}:{}", d.id, identity.sub))
}

/// `source`'s repositories, from memory unless `refresh`. When the forge
/// cannot be reached and a list was fetched earlier, that list comes back
/// with the offline notice; any other failure drops it.
pub async fn list(
    platform: &dyn Platform,
    http: &reqwest::Client,
    source: &ForgeSource,
    account: Option<&AccountDescriptor>,
    refresh: bool,
) -> Result<Listing, ForgeError> {
    let key = (
        source.id.clone(),
        owner_of_listing(platform, source, account)?,
    );
    let cached = CACHE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
        .cloned();
    if let (false, Some(cached)) = (refresh, &cached) {
        return Ok(cached.clone());
    }
    let fetched = fetch(platform, http, source, account, refresh).await;
    let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    match fetched {
        Ok(listing) => {
            cache.insert(key, listing.clone());
            Ok(listing)
        }
        Err(ForgeError::Unreachable(sentence)) => match cached {
            Some(mut cached) => {
                cached.notice(ListNotice::offline(&sentence, Some(cached.fetched_ms)));
                Ok(cached)
            }
            None => Err(ForgeError::Unreachable(sentence)),
        },
        Err(other) => {
            cache.remove(&key);
            Err(other)
        }
    }
}

async fn fetch(
    platform: &dyn Platform,
    http: &reqwest::Client,
    source: &ForgeSource,
    account: Option<&AccountDescriptor>,
    refresh: bool,
) -> Result<Listing, ForgeError> {
    match source.via {
        TokenVia::AccountForge => {
            let d = account.ok_or_else(|| {
                ForgeError::NeedsSignIn(
                    "Sign in to your account to see its repositories.".to_owned(),
                )
            })?;
            forgejo_listing(platform, http, source, d).await
        }
        TokenVia::DeviceFlow => github_user_listing(platform, http, source).await,
        TokenVia::Broker => {
            let d = account.ok_or_else(|| {
                ForgeError::NeedsSignIn(
                    "Sign in to your account to see its GitHub repositories.".to_owned(),
                )
            })?;
            broker_listing(platform, http, source, d, refresh).await
        }
    }
}

/// A next-page URL is followed with the token only on the source's own API
/// origin.
fn same_origin(next: &str, api_base: &str) -> bool {
    match (url::Url::parse(next), url::Url::parse(api_base)) {
        (Ok(next), Ok(base)) => next.origin() == base.origin(),
        _ => false,
    }
}

fn link_header(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get(reqwest::header::LINK)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// Which GitHub list is being paged.
#[derive(Clone, Copy)]
enum GithubList {
    /// `/user/repos`: each repository says whether the person may push.
    User,
    /// `/installation/repositories`: pushing is the broker's grant to say.
    Installation,
}

/// Page a GitHub list from `first`, following `rel="next"` on the source's
/// own origin: at most [`github::PAGE_CAP`] pages and at most `budget`
/// requests, either cap marking the listing truncated. A refused page ends
/// the list and is returned.
async fn github_pages(
    http: &reqwest::Client,
    source: &ForgeSource,
    first: String,
    token: &str,
    kind: GithubList,
    listing: &mut Listing,
    budget: &mut usize,
) -> Result<Option<GithubProblem>, ForgeError> {
    let mut next = Some(first);
    let mut pages = 0;
    while let Some(url) = next.take() {
        if pages == github::PAGE_CAP || *budget == 0 {
            listing.truncated = true;
            break;
        }
        pages += 1;
        *budget -= 1;
        let response = send(http, github::get(http, &url).bearer_auth(token)).await?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let link = link_header(&response);
        let bytes = body(response).await?;
        if status != 200 {
            return Ok(Some(github::classify_error(status, &headers, &bytes)));
        }
        if github::sso_partial(&headers) {
            listing.notice(ListNotice::sso(None));
        }
        match kind {
            GithubList::User => listing.repos.extend(github::parse_repos(&bytes)?),
            GithubList::Installation => {
                let page = github::parse_installation_repos(&bytes)?;
                for user in page.user_owners {
                    if !listing.you.contains(&user) {
                        listing.you.push(user);
                    }
                }
                listing.repos.extend(page.repos);
            }
        }
        next = link
            .as_deref()
            .and_then(github::next_link)
            .filter(|next| same_origin(next, &source.api_base));
    }
    Ok(None)
}

/// `/user/repos` with the person's own device-flow token. A connection
/// whose login GitHub did not say yet learns it here.
async fn github_user_listing(
    platform: &dyn Platform,
    http: &reqwest::Client,
    source: &ForgeSource,
) -> Result<Listing, ForgeError> {
    let token = tokens::device_token(platform, http, source).await?;
    let login = match tokens::load_session(platform, source)? {
        Some(mut stored) if stored.login.is_empty() => {
            match device_flow::user_login(http, source, &token).await {
                Ok(login) => {
                    stored.login = login;
                    tokens::store_session(platform, source, &stored)?;
                    Some(stored.login)
                }
                Err(_) => None,
            }
        }
        stored => stored.map(|stored| stored.login),
    };
    let mut listing = Listing::new(login);
    let mut budget = usize::MAX;
    let first = github::repos_url(&source.api_base);
    match github_pages(
        http,
        source,
        first,
        &token,
        GithubList::User,
        &mut listing,
        &mut budget,
    )
    .await?
    {
        None => {}
        Some(GithubProblem::Restricted) => listing.notice(ListNotice::restricted(source)),
        Some(GithubProblem::Sso(link)) => listing.notice(ListNotice::sso(link)),
        Some(GithubProblem::Error(ForgeError::NeedsConnect)) => {
            tokens::forge_disconnect(platform, source)?;
            return Err(ForgeError::NeedsConnect);
        }
        Some(GithubProblem::Error(error)) => return Err(error),
    }
    Ok(listing.finish())
}

/// Every owner the broker grants (at most [`OWNER_CAP`]), one installation
/// token each, within [`REQUEST_CAP`] requests. A refusal for one owner is
/// a notice; the other owners still list.
async fn broker_listing(
    platform: &dyn Platform,
    http: &reqwest::Client,
    source: &ForgeSource,
    d: &AccountDescriptor,
    refresh: bool,
) -> Result<Listing, ForgeError> {
    let broker_host = d
        .github_broker
        .as_ref()
        .map(|b| b.host())
        .unwrap_or_default();
    let mut budget = REQUEST_CAP - 1;
    let answer = match broker::whoami(platform, http, d, refresh).await {
        Ok(answer) => answer,
        Err(BrokerFailure::Error(error)) => return Err(error),
        Err(other) => return Err(ForgeError::Internal(format!("whoami answered {other:?}"))),
    };
    if answer.grants.is_empty() {
        return match source.own_via() {
            Some(TokenVia::DeviceFlow) => github_user_listing(platform, http, source).await,
            _ => Err(ForgeError::NoAccess(broker::no_access(&broker_host))),
        };
    }
    let mut listing = Listing::new(None);
    listing.you.clear();
    let mut owners = answer.owners();
    if owners.len() > OWNER_CAP {
        owners.truncate(OWNER_CAP);
        listing.truncated = true;
        listing.notice(ListNotice::owners_capped(&broker_host));
    }
    for owner in owners {
        if listing.repos.len() >= REPO_CAP || budget == 0 {
            listing.truncated = true;
            break;
        }
        budget -= 1;
        let token = match broker::mint(platform, http, d, &owner, None, broker::LIST_ASKS).await {
            Ok(token) => token,
            Err(BrokerFailure::Owner(problem, app)) => {
                listing.notice(ListNotice::new(
                    problem.sentence(&owner, app.as_deref(), &broker_host),
                    None,
                ));
                continue;
            }
            Err(BrokerFailure::NoGrants) => continue,
            Err(BrokerFailure::Error(error)) => return Err(error),
        };
        let first = github::installation_repos_url(&source.api_base);
        let start = listing.repos.len();
        let problem = github_pages(
            http,
            source,
            first,
            &token,
            GithubList::Installation,
            &mut listing,
            &mut budget,
        )
        .await?;
        for repo in &mut listing.repos[start..] {
            repo.can_push =
                repo.can_push && broker::can_write(&answer.grants, &repo.owner, &repo.name);
        }
        match problem {
            None => {}
            Some(GithubProblem::Error(ForgeError::Unreachable(sentence))) => {
                return Err(ForgeError::Unreachable(sentence))
            }
            Some(GithubProblem::Error(ForgeError::NeedsConnect)) => {
                // An installation token GitHub refuses is dead for every
                // drive too.
                broker::forget_token(&token);
                listing.notice(ListNotice::token_refused(&owner, &broker_host));
            }
            Some(GithubProblem::Error(error)) => {
                listing.notice(ListNotice::new(
                    format!("keeper couldn't list {owner}'s repositories: {error}"),
                    None,
                ));
            }
            Some(GithubProblem::Restricted) => listing.notice(ListNotice::restricted(source)),
            Some(GithubProblem::Sso(link)) => listing.notice(ListNotice::sso(link)),
        }
    }
    Ok(listing.finish())
}

/// The account's forge: UserInfo for the numeric id, then `repos/search`
/// page by page.
async fn forgejo_listing(
    platform: &dyn Platform,
    http: &reqwest::Client,
    source: &ForgeSource,
    d: &AccountDescriptor,
) -> Result<Listing, ForgeError> {
    let token = tokens::forge_token(platform, http, source, Some(d)).await?;
    let response = send(
        http,
        http.get(forgejo::userinfo_url(&source.web_base))
            .bearer_auth(&token),
    )
    .await?;
    let status = response.status().as_u16();
    let info = body(response).await?;
    if status != 200 {
        return Err(forgejo_refusal(source, status));
    }
    let uid = forgejo::parse_userinfo_sub(&info).ok_or_else(|| {
        ForgeError::Refused(format!("{} did not say who you are.", source.host()))
    })?;
    let login = session::load_bound_forge(platform, d)
        .ok()
        .flatten()
        .map(|stored| stored.login)
        .or_else(|| forgejo::parse_userinfo_login(&info));
    let mut listing = Listing::new(login);
    forgejo_pages(http, source, &token, &uid, &mut listing).await?;
    Ok(listing.finish())
}

fn forgejo_refusal(source: &ForgeSource, status: u16) -> ForgeError {
    match status {
        401 => ForgeError::NeedsSignIn("Reconnect the repository.".to_owned()),
        500..=599 => {
            ForgeError::Unreachable(format!("{} isn't answering right now.", source.host()))
        }
        _ => ForgeError::Refused(format!(
            "{} refused the list (HTTP {status}).",
            source.host()
        )),
    }
}

/// `repos/search` from page 1: as many pages as `X-Total-Count` says, or,
/// without it, while a page comes back full — at most
/// [`forgejo::PAGE_CAP`], past which the listing is truncated.
async fn forgejo_pages(
    http: &reqwest::Client,
    source: &ForgeSource,
    token: &str,
    uid: &str,
    listing: &mut Listing,
) -> Result<(), ForgeError> {
    let mut last: Option<u64> = None;
    let mut page = 1;
    loop {
        let url = forgejo::search_url(&source.api_base, uid, page);
        let response = send(http, http.get(url).bearer_auth(token)).await?;
        let status = response.status().as_u16();
        let total = forgejo::total_count(response.headers());
        let bytes = body(response).await?;
        if status != 200 {
            return Err(forgejo_refusal(source, status));
        }
        if page == 1 {
            if let Some(total) = total {
                let (pages, truncated) = forgejo::pages(total);
                last = Some(pages);
                listing.truncated = truncated;
            }
        }
        let repos = forgejo::parse_search(&bytes)?;
        let full = repos.len() as u64 >= forgejo::PAGE_SIZE;
        listing.repos.extend(repos);
        let more = match last {
            Some(last) => page < last,
            None => full,
        };
        if !more {
            return Ok(());
        }
        if page == forgejo::PAGE_CAP {
            listing.truncated = true;
            return Ok(());
        }
        page += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forges::testing::{self, Reply};
    use crate::forges::tokens::{store_session, StoredForgeToken};
    use crate::forges::ForgeKind;
    use crate::org_account::session as account_session;

    fn repo_json(owner: &str, name: &str, owner_type: &str) -> String {
        format!(
            r#"{{"name":"{name}","full_name":"{owner}/{name}","owner":{{"login":"{owner}","type":"{owner_type}"}},
                "clone_url":"https://github.com/{owner}/{name}.git","html_url":"https://github.com/{owner}/{name}",
                "default_branch":"main","private":true}}"#
        )
    }

    fn github_source(base: &str, id: &str, via: TokenVia) -> ForgeSource {
        ForgeSource {
            id: id.to_owned(),
            kind: ForgeKind::Github,
            name: "GitHub".to_owned(),
            web_base: base.to_owned(),
            api_base: base.to_owned(),
            client_id: Some("Iv1.c".to_owned()),
            via,
        }
    }

    fn connection(login: &str) -> StoredForgeToken {
        StoredForgeToken {
            access_token: "gho_t".to_owned(),
            refresh_token: None,
            expires_ms: None,
            login: login.to_owned(),
            client_id: "Iv1.c".to_owned(),
        }
    }

    fn next_page(seen: &testing::Seen, path: &str, page: u32) -> String {
        format!(
            "<http://{}{path}?page={page}>; rel=\"next\"",
            seen.header("host").unwrap_or_default()
        )
    }

    fn broker_account(p: &testing::FakePlatform, base: &str) -> AccountDescriptor {
        testing::signed_in_account(p, &format!(r#", "github_broker": {{ "url": "{base}" }}"#))
    }

    fn broker_source(base: &str, id: &str) -> ForgeSource {
        ForgeSource {
            client_id: None,
            ..github_source(base, id, TokenVia::Broker)
        }
    }

    fn count(fake: &testing::Fake, path: &str) -> usize {
        fake.requests()
            .iter()
            .filter(|seen| seen.path == path)
            .count()
    }

    #[tokio::test]
    async fn the_device_listing_follows_next_links_and_names_restricted_orgs() {
        let fake = testing::serve(|seen| {
            assert_eq!(seen.header("authorization"), Some("Bearer gho_t"));
            assert_eq!(seen.header("x-github-api-version"), Some("2022-11-28"));
            match seen.query.get("page").map(String::as_str) {
                None => {
                    assert_eq!(
                        seen.query.get("affiliation").map(String::as_str),
                        Some("owner,collaborator,organization_member")
                    );
                    assert!(!seen.query.contains_key("type"));
                    Reply::json(200, &format!("[{}]", repo_json("tgorka", "a", "User")))
                        .header("Link", &next_page(seen, "/user/repos", 2))
                        .header("X-GitHub-SSO", "partial-results; organizations=1")
                }
                Some("2") => Reply::json(
                    403,
                    r#"{"message":"the `acme` organization has enabled OAuth App access restrictions"}"#,
                ),
                other => panic!("unexpected page {other:?}"),
            }
        });
        let p = testing::FakePlatform::default();
        let source = github_source(&fake.base, "gh-list", TokenVia::DeviceFlow);
        store_session(&p, &source, &connection("tgorka")).expect("seed");
        let listing = list(&p, &testing::http(), &source, None, true)
            .await
            .expect("listing");
        assert_eq!(listing.login.as_deref(), Some("tgorka"));
        assert_eq!(listing.repos.len(), 1);
        let sentences: Vec<&str> = listing
            .notices
            .iter()
            .map(|n| n.sentence.as_str())
            .collect();
        assert!(sentences.iter().any(|s| s.contains("single sign-on")));
        let restricted = listing
            .notices
            .iter()
            .find(|n| n.sentence.contains("haven't approved keeper"))
            .expect("restricted notice");
        assert_eq!(
            restricted.link.as_deref(),
            Some(format!("{}/settings/connections/applications/Iv1.c", fake.base).as_str())
        );
        assert!(!listing.truncated);
    }

    #[tokio::test]
    async fn a_device_listing_stops_at_ten_pages_and_says_so() {
        let fake = testing::serve(|seen| {
            let page: u32 = seen
                .query
                .get("page")
                .and_then(|p| p.parse().ok())
                .unwrap_or(1);
            Reply::json(
                200,
                &format!("[{}]", repo_json("tgorka", &format!("r{page}"), "User")),
            )
            .header("Link", &next_page(seen, "/user/repos", page + 1))
        });
        let p = testing::FakePlatform::default();
        let source = github_source(&fake.base, "gh-cap", TokenVia::DeviceFlow);
        store_session(&p, &source, &connection("tgorka")).expect("seed");
        let listing = list(&p, &testing::http(), &source, None, true)
            .await
            .expect("listing");
        assert_eq!(fake.requests().len(), github::PAGE_CAP);
        assert_eq!(listing.repos.len(), github::PAGE_CAP);
        assert!(listing.truncated);
        assert_eq!(listing.notices, [ListNotice::truncated(github::PAGE_CAP)]);
    }

    #[tokio::test]
    async fn a_connection_without_a_login_learns_it_at_the_next_listing() {
        let fake = testing::serve(|seen| match seen.path.as_str() {
            "/user" => Reply::json(200, r#"{"login":"tgorka"}"#),
            _ => Reply::json(200, "[]"),
        });
        let p = testing::FakePlatform::default();
        let source = github_source(&fake.base, "gh-login", TokenVia::DeviceFlow);
        store_session(&p, &source, &connection("")).expect("seed");
        let listing = list(&p, &testing::http(), &source, None, true)
            .await
            .expect("listing");
        assert_eq!(listing.login.as_deref(), Some("tgorka"));
        let kept = tokens::load_session(&p, &source)
            .expect("read")
            .expect("kept");
        assert_eq!(kept.login, "tgorka");
    }

    #[tokio::test]
    async fn a_broker_listing_lists_each_owner_and_turns_one_owner_s_refusal_into_a_notice() {
        let fake = testing::serve(|seen| {
            match (seen.method.as_str(), seen.path.as_str()) {
            ("GET", "/v1/whoami") => Reply::json(
                200,
                r#"{"sub":"1","subject":"tomasz","grants":[
                    {"app":"tgbot","owners":["tgorka","ghost"],"repositories":"*",
                     "permissions":{"metadata":"read","contents":"write"}},
                    {"app":"tgdev","owners":["acme"],"repositories":"*",
                     "permissions":{"metadata":"read","contents":"read"}}]}"#,
            ),
            ("POST", "/v1/token") => {
                let body = seen.json();
                assert_eq!(
                    body["permissions"],
                    serde_json::json!({ "metadata": "read" })
                );
                assert!(body.get("repositories").is_none());
                match body["owner"].as_str() {
                    Some("ghost") => Reply::json(
                        404,
                        r#"{"error":"not_installed","detail":"app tgbot is not installed on ghost"}"#,
                    ),
                    Some(owner) => Reply::json(
                        200,
                        &format!(
                            r#"{{"token":"ghs_{owner}","expires_at":"2099-01-01T00:00:00Z"}}"#
                        ),
                    ),
                    None => Reply::json(400, r#"{"error":"bad_request"}"#),
                }
            }
            // api.github.com's own rule: no User-Agent, a plain-text 403. keeper
            // shipped without one once, and every owner read "GitHub refused
            // the list (HTTP 403)".
            ("GET", "/installation/repositories") if seen.header("user-agent").is_none() => {
                Reply::json(
                    403,
                    "Request forbidden by administrative rules. Please make sure your request has a User-Agent header",
                )
            }
            ("GET", "/installation/repositories") => match seen.header("authorization") {
                Some("Bearer ghs_tgorka") => Reply::json(
                    200,
                    &format!(
                        r#"{{"total_count":1,"repositories":[{}]}}"#,
                        repo_json("tgorka", "notes", "User")
                    ),
                ),
                Some("Bearer ghs_acme") => Reply::json(
                    200,
                    &format!(
                        r#"{{"total_count":2,"repositories":[{},{}]}}"#,
                        repo_json("acme", "site", "Organization"),
                        repo_json("acme", "api", "Organization")
                    ),
                ),
                other => panic!("unexpected token {other:?}"),
            },
            _ => Reply::json(404, r#"{"error":"not_found"}"#),
        }
        });
        let p = testing::FakePlatform::default();
        let d = broker_account(&p, &fake.base);
        let source = broker_source(&fake.base, "gh-broker");
        let listing = list(&p, &testing::http(), &source, Some(&d), true)
            .await
            .expect("listing");
        let rows: Vec<(&str, bool)> = listing
            .repos
            .iter()
            .map(|r| (r.full_name.as_str(), r.pull_only()))
            .collect();
        // You first; acme's grant reads only, so its drives only download.
        assert_eq!(
            rows,
            [
                ("tgorka/notes", false),
                ("acme/api", true),
                ("acme/site", true)
            ]
        );
        assert_eq!(listing.you, ["tgorka"]);
        assert_eq!(
            listing.notices,
            [ListNotice::new(
                "keeper's GitHub app tgbot isn't installed on ghost.",
                None
            )]
        );
    }

    #[tokio::test]
    async fn no_grants_is_no_access_and_never_serves_an_old_list() {
        let granted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let grants = std::sync::Arc::clone(&granted);
        let fake = testing::serve(move |seen| match seen.path.as_str() {
            "/v1/whoami" if grants.load(std::sync::atomic::Ordering::SeqCst) => Reply::json(
                200,
                r#"{"sub":"1","subject":"t","grants":[{"app":"tgbot","owners":["tgorka"],
                    "repositories":"*","permissions":{"metadata":"read"}}]}"#,
            ),
            "/v1/whoami" => Reply::json(200, r#"{"sub":"1","subject":null,"grants":[]}"#),
            "/v1/token" => Reply::json(
                200,
                r#"{"token":"ghs_t","expires_at":"2099-01-01T00:00:00Z"}"#,
            ),
            _ => Reply::json(
                200,
                &format!(
                    r#"{{"total_count":1,"repositories":[{}]}}"#,
                    repo_json("tgorka", "notes", "User")
                ),
            ),
        });
        let p = testing::FakePlatform::default();
        let d = broker_account(&p, &fake.base);
        let source = broker_source(&fake.base, "gh-nogrant");
        let http = testing::http();
        assert_eq!(
            list(&p, &http, &source, Some(&d), true)
                .await
                .expect("granted")
                .repos
                .len(),
            1
        );
        granted.store(false, std::sync::atomic::Ordering::SeqCst);
        let no_access = Err(ForgeError::NoAccess(
            "Your account has no GitHub access on 127.0.0.1. Ask its administrator to add you."
                .to_owned(),
        ));
        assert_eq!(list(&p, &http, &source, Some(&d), true).await, no_access);
        assert_eq!(list(&p, &http, &source, Some(&d), false).await, no_access);
        // With a device-flow client id, the fallback is to connect.
        let fallback = github_source(&fake.base, "gh-nogrant-fallback", TokenVia::Broker);
        assert_eq!(
            list(&p, &http, &fallback, Some(&d), true).await,
            Err(ForgeError::NeedsConnect)
        );
    }

    #[tokio::test]
    async fn a_listing_is_served_only_to_the_identity_that_fetched_it() {
        let fake = testing::serve(|seen| match seen.path.as_str() {
            "/v1/whoami" => Reply::json(
                200,
                r#"{"sub":"1","subject":"t","grants":[{"app":"tgbot","owners":["tgorka"],
                    "repositories":"*","permissions":{"metadata":"read"}}]}"#,
            ),
            "/v1/token" => Reply::json(
                200,
                r#"{"token":"ghs_t","expires_at":"2099-01-01T00:00:00Z"}"#,
            ),
            _ => Reply::json(
                200,
                &format!(
                    r#"{{"total_count":1,"repositories":[{}]}}"#,
                    repo_json("tgorka", "private-notes", "User")
                ),
            ),
        });
        let p = testing::FakePlatform::default();
        let d = broker_account(&p, &fake.base);
        let source = broker_source(&fake.base, "gh-identity");
        let http = testing::http();
        list(&p, &http, &source, Some(&d), true)
            .await
            .expect("first person");
        list(&p, &http, &source, Some(&d), false)
            .await
            .expect("from memory");
        assert_eq!(count(&fake, "/installation/repositories"), 1);

        let mut other = account_session::load_session(&p, &d.id)
            .expect("read")
            .expect("session");
        other.sub = "sub-2".to_owned();
        account_session::store_session(&p, &d.id, &other).expect("store");
        list(&p, &http, &source, Some(&d), false)
            .await
            .expect("second person");
        assert_eq!(
            count(&fake, "/installation/repositories"),
            2,
            "another person's list is fetched for them"
        );

        p.keychain_delete(&account_session::session_key(&d.id))
            .expect("sign out");
        assert!(matches!(
            list(&p, &http, &source, Some(&d), false).await,
            Err(ForgeError::NeedsSignIn(_))
        ));
    }

    /// A broker whose grants name `owners` owners, each with `pages` pages
    /// of one repository (unbounded when `None`).
    fn many_owners(owners: usize, pages: Option<u32>) -> testing::Fake {
        let names: Vec<String> = (0..owners).map(|n| format!("\"o{n:02}\"")).collect();
        let whoami = format!(
            r#"{{"sub":"1","subject":"t","grants":[{{"app":"tgbot","owners":[{}],
                "repositories":"*","permissions":{{"metadata":"read"}}}}]}}"#,
            names.join(",")
        );
        testing::serve(move |seen| match seen.path.as_str() {
            "/v1/whoami" => Reply::json(200, &whoami),
            "/v1/token" => Reply::json(
                200,
                &format!(
                    r#"{{"token":"ghs_{}","expires_at":"2099-01-01T00:00:00Z"}}"#,
                    seen.json()["owner"].as_str().unwrap_or_default()
                ),
            ),
            _ => {
                let owner = seen
                    .header("authorization")
                    .and_then(|a| a.strip_prefix("Bearer ghs_"))
                    .unwrap_or_default()
                    .to_owned();
                let page: u32 = seen
                    .query
                    .get("page")
                    .and_then(|p| p.parse().ok())
                    .unwrap_or(1);
                let reply = Reply::json(
                    200,
                    &format!(
                        r#"{{"total_count":1,"repositories":[{}]}}"#,
                        repo_json(&owner, &format!("r{page}"), "Organization")
                    ),
                );
                if pages.is_none_or(|last| page < last) {
                    reply.header(
                        "Link",
                        &next_page(seen, "/installation/repositories", page + 1),
                    )
                } else {
                    reply
                }
            }
        })
    }

    #[tokio::test]
    async fn a_broker_listing_asks_about_twenty_owners_at_most() {
        let fake = many_owners(25, Some(1));
        let p = testing::FakePlatform::default();
        let d = broker_account(&p, &fake.base);
        let source = broker_source(&fake.base, "gh-owners");
        let listing = list(&p, &testing::http(), &source, Some(&d), true)
            .await
            .expect("listing");
        assert_eq!(count(&fake, "/v1/token"), OWNER_CAP);
        assert_eq!(listing.repos.len(), OWNER_CAP);
        assert!(listing.truncated);
        assert!(listing
            .notices
            .contains(&ListNotice::owners_capped("127.0.0.1")));
    }

    #[tokio::test]
    async fn a_broker_listing_sends_sixty_requests_at_most() {
        let fake = many_owners(OWNER_CAP, None);
        let p = testing::FakePlatform::default();
        let d = broker_account(&p, &fake.base);
        let source = broker_source(&fake.base, "gh-requests");
        let listing = list(&p, &testing::http(), &source, Some(&d), true)
            .await
            .expect("listing");
        assert_eq!(fake.requests().len(), REQUEST_CAP);
        assert!(listing.truncated);
    }

    #[tokio::test]
    async fn an_installation_token_github_refuses_is_dropped_with_a_broker_notice() {
        let fake = testing::serve(|seen| match seen.path.as_str() {
            "/v1/whoami" => Reply::json(
                200,
                r#"{"sub":"1","subject":"t","grants":[{"app":"tgbot","owners":["acme"],
                    "repositories":"*","permissions":{"metadata":"read"}}]}"#,
            ),
            "/v1/token" => Reply::json(
                200,
                r#"{"token":"ghs_dead","expires_at":"2099-01-01T00:00:00Z"}"#,
            ),
            _ => Reply::json(401, r#"{"message":"Bad credentials"}"#),
        });
        let p = testing::FakePlatform::default();
        let d = broker_account(&p, &fake.base);
        let source = broker_source(&fake.base, "gh-dead");
        let http = testing::http();
        let listing = list(&p, &http, &source, Some(&d), true)
            .await
            .expect("listing");
        assert_eq!(
            listing.notices,
            [ListNotice::token_refused("acme", "127.0.0.1")]
        );
        list(&p, &http, &source, Some(&d), true)
            .await
            .expect("again");
        assert_eq!(
            count(&fake, "/v1/token"),
            2,
            "the refused token is not reused"
        );
    }

    fn search_page(from: usize, n: usize) -> String {
        let repos: Vec<String> = (from..from + n)
            .map(|i| {
                format!(
                    r#"{{"name":"r{i}","full_name":"me/r{i}","owner":{{"login":"me"}},
                        "clone_url":"https://git.acme.dev/me/r{i}.git","html_url":"https://git.acme.dev/me/r{i}"}}"#
                )
            })
            .collect();
        format!(r#"{{"ok":true,"data":[{}]}}"#, repos.join(","))
    }

    #[tokio::test]
    async fn forgejo_pages_while_pages_come_back_full_and_stops_at_twenty() {
        let size = forgejo::PAGE_SIZE as usize;
        let endless = testing::serve(move |_| Reply::json(200, &search_page(0, size)));
        let source = github_source(&endless.base, "cb", TokenVia::AccountForge);
        let mut listing = Listing::new(None);
        forgejo_pages(&testing::http(), &source, "t", "7", &mut listing)
            .await
            .expect("pages");
        assert_eq!(endless.requests().len(), forgejo::PAGE_CAP as usize);
        assert!(listing.truncated);

        // No `X-Total-Count`: a short page is the last one.
        let short = testing::serve(
            move |seen| match seen.query.get("page").map(String::as_str) {
                Some("1") => Reply::json(200, &search_page(0, size)),
                _ => Reply::json(200, &search_page(size, 3)),
            },
        );
        let source = github_source(&short.base, "cb", TokenVia::AccountForge);
        let mut listing = Listing::new(None);
        forgejo_pages(&testing::http(), &source, "t", "7", &mut listing)
            .await
            .expect("pages");
        assert_eq!(short.requests().len(), 2);
        assert_eq!(listing.repos.len(), size + 3);
        assert!(!listing.truncated);
    }

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
            clone_url: String::new(),
            web_url: String::new(),
            updated_ms: None,
            size_kb: None,
            can_push: true,
        }
    }

    #[test]
    fn a_long_list_keeps_your_own_repositories_and_says_it_was_cut() {
        let mut listing = Listing::new(Some("me".to_owned()));
        listing.repos = vec![repo("acme", "r"); REPO_CAP];
        listing.repos.push(repo("me", "mine"));
        let listing = listing.finish();
        assert_eq!(listing.repos.len(), REPO_CAP);
        assert_eq!(listing.repos[0].full_name, "me/mine");
        assert!(listing.truncated);
        assert_eq!(
            listing.notices,
            [ListNotice::new(
                "Only the first 1,000 are listed; search looks only through those.",
                None
            )]
        );
    }

    #[test]
    fn a_repository_only_downloads_when_keeper_cannot_or_should_not_push() {
        let writable = repo("me", "r");
        assert!(!writable.pull_only());
        assert_eq!(writable.pull_only_sentence(), None);
        for (repo, sentence) in [
            (
                ForgeRepo {
                    can_push: false,
                    ..writable.clone()
                },
                "You can only read this repository, so keeper only downloads it.",
            ),
            (
                ForgeRepo {
                    archived: true,
                    ..writable.clone()
                },
                "This repository is archived, so keeper only downloads it.",
            ),
            (
                ForgeRepo {
                    mirror: true,
                    ..writable.clone()
                },
                "This repository is a mirror, so keeper only downloads it.",
            ),
        ] {
            assert!(repo.pull_only(), "{sentence}");
            assert_eq!(repo.pull_only_sentence(), Some(sentence));
        }
    }
}
