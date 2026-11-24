//! One call that gives a module a [`Source`] over the replica its test
//! chooses: the fake, or a real replica the test's own opener opens on a
//! temporary file. keeper-testkit never depends on keeper-replica; a test
//! that wants the real one passes its opener in.

use std::path::PathBuf;
use std::sync::Arc;

use keeper_ports::{Link, Opener, ReplicaSpec, Source};

use crate::{blob_server, bootstrapped, pins, queued, FakeOpener, FakeReplica, Seen};

/// The database every harness opens.
pub const DATABASE: &str = "alpha";
/// The person the harness's source speaks for.
pub const ACTOR: &str = "ana";
/// The device the harness's source is.
pub const CLIENT: &str = "ana.1";

/// The replica a module's test runs over.
pub enum Over<'a> {
    /// A [`FakeReplica`]: no file and no network.
    Fake,
    /// A real replica `opener` opens on a temporary file, on this device
    /// alone (`link: None`): no upload server, no request.
    Local(&'a dyn Opener),
    /// A real replica `opener` opens on a temporary file, linked to the
    /// blob-route stand-in ([`blob_server`]) and bootstrapped, so its rounds
    /// upload what is staged. The stand-in answers each `(blob, status)` of
    /// the list with that status; every other upload it takes, and
    /// everything else — the push included — it answers `503`.
    Linked(&'a dyn Opener, Vec<(String, u16)>),
}

/// A module's [`Source`] and what its test observes the replica by.
pub struct Harness {
    pub source: Source,
    /// The replica file (the fake writes none).
    pub path: PathBuf,
    /// The fake, under [`Over::Fake`].
    pub fake: Option<Arc<FakeReplica>>,
    /// The stand-in's request record, under [`Over::Linked`].
    pub seen: Option<Seen>,
    _dir: tempfile::TempDir,
}

impl Harness {
    /// The bodies staged for upload, sorted: the fake's, or the file's.
    pub fn queued(&self) -> Vec<String> {
        match &self.fake {
            Some(fake) => fake.queued(),
            None => queued(&self.path),
        }
    }

    /// The commits that pin each blob, `(commit, blob)`, sorted: the
    /// fake's, or the file's.
    pub fn pins(&self) -> Vec<(String, String)> {
        match &self.fake {
            Some(fake) => fake.pins(),
            None => pins(&self.path),
        }
    }
}

impl Drop for Harness {
    /// The replica stops and lets its file go before the directory does.
    fn drop(&mut self) {
        self.source.replica.close();
        self.source.replica.join();
    }
}

/// A [`Source`] for [`DATABASE`], as [`ACTOR`] on [`CLIENT`], over the
/// replica `over` names.
pub fn harness(over: Over<'_>) -> Harness {
    harness_of(DATABASE, over)
}

/// [`harness`] for a database the test names (a graph's domain is its
/// database).
pub fn harness_of(database: &str, over: Over<'_>) -> Harness {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join(format!("{database}.sqlite"));
    let spec = |link: Option<Link>| ReplicaSpec {
        database: database.to_owned(),
        path: path.clone(),
        client_id: Some(CLIENT.to_owned()),
        link,
        units: Vec::new(),
        watch_presence: None,
    };
    let (replica, fake, seen) = match over {
        Over::Fake => {
            let opener = FakeOpener::default();
            let replica = opener.open(spec(None)).expect("the fake opens");
            let fake = opener.all().pop().expect("the fake it opened");
            (replica, Some(fake), None)
        }
        Over::Local(opener) => (opener.open(spec(None)).expect("opens"), None, None),
        Over::Linked(opener, refused) => {
            let (port, seen) = blob_server(refused);
            let link = Link {
                base_url: format!("http://127.0.0.1:{port}/{database}"),
                token: "0123456789abcdef".to_owned(),
                store: Default::default(),
            };
            let replica = opener.open(spec(Some(link))).expect("opens");
            bootstrapped(&path);
            (replica, None, Some(seen))
        }
    };
    Harness {
        source: Source {
            database: database.to_owned(),
            replica,
            actor: ACTOR.to_owned(),
            client: CLIENT.to_owned(),
        },
        path,
        fake,
        seen,
        _dir: dir,
    }
}
