//! The voices bank (AD-343, AD-346, AD-347): people, their voice clips, one
//! embedding per clip per embedding model, the dictionary and tombstones —
//! one file per fact inside `<drive>/<voices subfolder>/`, so two devices
//! adding to it at once sync as a union, never as a conflict.
//!
//! This module reads the bank and PLANS writes; the shell executes a
//! [`BankPlan`] (writes, then deletes) inside the drive, and sync carries it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::model::{Candidate, MatchStatus};
use super::models::is_plain_segment;
use super::words::{fold, same_folded};

pub const PEOPLE_DIR: &str = "people";
pub const CLIPS_DIR: &str = "clips";
pub const EMBEDDINGS_DIR: &str = "embeddings";
pub const DICTIONARY_DIR: &str = "dictionary";
pub const TOMBSTONES_DIR: &str = "tombstones";

/// The `version` every bank file carries.
pub const BANK_FILE_VERSION: u32 = 1;

/// Cosine at or above which a speaker is assigned to a person (AD-346).
/// Uncalibrated.
pub const AUTO_MATCH: f32 = 0.70;
/// Cosine at or above which a person is offered as a candidate (AD-346).
/// Uncalibrated.
pub const SUGGEST: f32 = 0.50;

/// Clips are 16 kHz mono PCM16.
pub const WAV_SAMPLE_RATE: u32 = 16_000;

/// Two samples are one span of one transcript when their start and end each
/// agree within this many seconds.
pub const SOURCE_TOLERANCE_S: f64 = 0.01;

const MAX_CANDIDATES: usize = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub version: u32,
    /// ULID.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    /// The person using this bank's drives: the microphone's voice.
    #[serde(rename = "self", default)]
    pub is_self: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingSample {
    pub version: u32,
    /// The embedding model id — also the folder the file lives under.
    pub model: String,
    pub person: String,
    pub clip: String,
    pub vector: Vec<f32>,
    pub source: SampleSource,
    pub added_at: String,
}

/// Where a sample was heard: the transcript (relative to the drive) and span.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SampleSource {
    pub transcript: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryTerm {
    pub version: u32,
    /// ULID.
    pub id: String,
    /// The spelling the transcript should carry.
    pub text: String,
    /// What the recognizer writes instead; matched whole-word, any case.
    #[serde(default)]
    pub aliases: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tombstone {
    pub id: String,
    pub deleted_at: String,
    /// Set when the person was merged into another: whatever still arrives
    /// under the merged-away id (a device that had not heard of the merge)
    /// belongs to this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_into: Option<String>,
}

/// Where a clip stored without an embedding was heard — the record
/// `clips/<person>/<clip>.json` beside its WAV, so the clip can be recognised
/// as the same span again before any model has embedded it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipRecord {
    pub version: u32,
    pub person: String,
    pub clip: String,
    pub source: SampleSource,
}

/// A clip present in the bank.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipFile {
    pub person: String,
    pub clip: String,
    /// Relative to the bank root, `/`-separated.
    pub rel_path: String,
    /// Where it was heard, from its [`ClipRecord`] when it has one.
    pub source: Option<SampleSource>,
}

/// A file to write, relative to the bank root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BankWrite {
    pub rel_path: String,
    pub bytes: Vec<u8>,
}

/// A file to delete, relative to the bank root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BankDelete {
    pub rel_path: String,
}

/// What a bank edit does on disk. Writes first, then deletes: a crash in
/// between leaves a duplicate, never a loss.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BankPlan {
    pub writes: Vec<BankWrite>,
    pub deletes: Vec<BankDelete>,
}

impl BankPlan {
    /// `other`'s writes after this plan's, and its deletes after this plan's.
    pub fn extend(&mut self, other: Self) {
        self.writes.extend(other.writes);
        self.deletes.extend(other.deletes);
    }
}

/// Who a confirmed speaker is: a person already in the bank, or one to make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Naming {
    Existing(String),
    New(String),
}

/// A confirmed speaker's voice, cut from its media before the bank is
/// touched.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceSample {
    /// 16 kHz mono PCM16 WAV bytes.
    pub wav: Vec<u8>,
    /// `None` while a transcription job holds the engine: the clip is kept
    /// alone and the next job's re-embed gives it its embedding.
    pub embedding: Option<Vec<f32>>,
    pub source: SampleSource,
}

/// [`Bank::plan_confirmation`]'s answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Confirmation {
    pub person: Person,
    pub plan: BankPlan,
    /// What the plan leaves out and why, for the log.
    pub left_out: Vec<String>,
}

/// The bank's answer for one speaker embedding.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchResult {
    pub status: MatchStatus,
    pub person_id: Option<String>,
    pub name: Option<String>,
    pub score: Option<f32>,
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BankError {
    #[error("Nobody with id {0} is in the voices bank.")]
    UnknownPerson(String),
    #[error("A name cannot be empty.")]
    EmptyName,
    #[error("Someone cannot be merged into themselves.")]
    SamePerson,
    #[error("{0} is not an embedding model id.")]
    InvalidModel(String),
    #[error("A dictionary term needs its text.")]
    EmptyTerm,
    #[error("An empty voice sample cannot be stored.")]
    EmptySample,
    #[error("Nothing with id {0} is in the dictionary.")]
    UnknownTerm(String),
    #[error("The voice clip is not a 16 kHz mono PCM16 WAV: {0}")]
    Wav(String),
    #[error("{path}: {message}")]
    Io { path: String, message: String },
    #[error("{0} was written by a newer keeper; it is left as it is.")]
    NewerFile(String),
}

/// The bank as read from disk.
#[derive(Debug, Clone, Default)]
pub struct Bank {
    root: PathBuf,
    /// Live people, by name.
    pub people: Vec<Person>,
    /// Every model's samples of live people — including those still filed
    /// under a person merged into them ([`Tombstone::merged_into`]).
    pub samples: Vec<EmbeddingSample>,
    pub clips: Vec<ClipFile>,
    pub terms: Vec<DictionaryTerm>,
    pub tombstones: Vec<Tombstone>,
    /// Files skipped, one sentence each.
    pub warnings: Vec<String>,
    /// Every person whose file says `self`, before the one-self rule.
    self_claims: Vec<String>,
    /// Files a newer keeper wrote, relative to the root: never rewritten.
    newer: HashSet<String>,
    /// `(model, person, clip)` → the file of a sample re-homed onto a merge
    /// survivor, which still sits under the merged-away id.
    rehomed: HashMap<(String, String, String), String>,
}

impl Bank {
    /// Read the bank under `root`. Never fails: an absent root is an empty
    /// bank and an unreadable or foreign file is a warning, because one bad
    /// file synced from another device must not blind every transcript. A
    /// file a newer keeper wrote (`version` above [`BANK_FILE_VERSION`]) is
    /// skipped with a warning, and no planner rewrites it
    /// ([`BankError::NewerFile`]).
    ///
    /// A tombstone wins over a person file, whenever either arrived: a device
    /// that had not heard of the deletion re-syncing the person's files does
    /// not resurrect them. Clips and embeddings that arrive under a person
    /// merged into another count as the survivor's (following merges of
    /// merges). Two people claiming `self` (set on two devices at once)
    /// resolve to the most recently updated.
    pub fn load(root: &Path) -> Self {
        let mut warnings = Vec::new();
        let mut newer = HashSet::new();
        let tombstones: Vec<Tombstone> =
            read_json_dir(root, TOMBSTONES_DIR, &mut warnings, &mut newer)
                .into_iter()
                .filter_map(|(stem, tombstone): (String, Tombstone)| {
                    keep_if_named(stem, tombstone, |t| &t.id, TOMBSTONES_DIR, &mut warnings)
                })
                .collect();
        let dead: HashSet<&str> = tombstones.iter().map(|t| t.id.as_str()).collect();

        let mut people: Vec<Person> = read_json_dir(root, PEOPLE_DIR, &mut warnings, &mut newer)
            .into_iter()
            .filter_map(|(stem, person): (String, Person)| {
                keep_if_named(stem, person, |p| &p.id, PEOPLE_DIR, &mut warnings)
            })
            .filter(|person| !dead.contains(person.id.as_str()))
            .collect();
        people.sort_by(|a, b| {
            fold(&a.name)
                .cmp(&fold(&b.name))
                .then_with(|| a.id.cmp(&b.id))
        });
        let self_claims: Vec<String> = people
            .iter()
            .filter(|person| person.is_self)
            .map(|person| person.id.clone())
            .collect();
        if self_claims.len() > 1 {
            let winner = people
                .iter()
                .filter(|person| person.is_self)
                .max_by(|a, b| {
                    a.updated_at
                        .cmp(&b.updated_at)
                        .then_with(|| a.id.cmp(&b.id))
                })
                .map(|person| person.id.clone());
            for person in &mut people {
                person.is_self = Some(&person.id) == winner.as_ref();
            }
            warnings.push(format!(
                "{} people are marked as you; the most recently changed one counts.",
                self_claims.len()
            ));
        }
        let live: HashSet<&str> = people.iter().map(|p| p.id.as_str()).collect();
        let merged: HashMap<&str, &str> = tombstones
            .iter()
            .filter_map(|t| Some((t.id.as_str(), t.merged_into.as_deref()?)))
            .collect();
        // The live person a folder's files belong to: its own, or the end of
        // its chain of merges. A chain that loops or ends in a deletion owns
        // nothing.
        let owner = |dir: &str| -> Option<String> {
            let mut current = dir;
            let mut seen = HashSet::new();
            loop {
                if live.contains(current) {
                    return Some(current.to_owned());
                }
                if !seen.insert(current) {
                    return None;
                }
                current = merged.get(current)?;
            }
        };
        // Each folder with its owner, a person's own folders first: a file
        // the survivor already holds under its own id wins over the copy
        // still filed under a merged-away one.
        let owned = |dirs: Vec<String>| -> Vec<(String, String)> {
            let mut owned: Vec<(String, String)> = dirs
                .into_iter()
                .filter_map(|dir| Some((owner(&dir)?, dir)))
                .collect();
            owned.sort_by_key(|(person, dir)| person != dir);
            owned
        };

        let mut clips: Vec<ClipFile> = Vec::new();
        for (person, person_dir) in owned(list_dirs(&root.join(CLIPS_DIR), &mut warnings)) {
            let dir = format!("{CLIPS_DIR}/{person_dir}");
            let records: HashMap<String, ClipRecord> =
                read_json_dir(root, &dir, &mut warnings, &mut newer)
                    .into_iter()
                    .filter(|(stem, record): &(String, ClipRecord)| {
                        record.clip == *stem && record.person == person_dir
                    })
                    .collect();
            for (stem, file) in list_files(&root.join(&dir), "wav", &mut warnings) {
                if person != person_dir
                    && clips.iter().any(|c| c.person == person && c.clip == stem)
                {
                    continue;
                }
                clips.push(ClipFile {
                    rel_path: format!("{dir}/{file}"),
                    person: person.clone(),
                    source: records.get(&stem).map(|record| record.source.clone()),
                    clip: stem,
                });
            }
        }

        let mut samples: Vec<EmbeddingSample> = Vec::new();
        let mut rehomed = HashMap::new();
        for model in list_dirs(&root.join(EMBEDDINGS_DIR), &mut warnings) {
            let model_dir = format!("{EMBEDDINGS_DIR}/{model}");
            for (person, person_dir) in owned(list_dirs(&root.join(&model_dir), &mut warnings)) {
                let dir = format!("{model_dir}/{person_dir}");
                for (stem, mut sample) in
                    read_json_dir::<EmbeddingSample>(root, &dir, &mut warnings, &mut newer)
                {
                    if sample.clip != stem || sample.person != person_dir || sample.model != model {
                        warnings.push(format!(
                            "{dir}/{stem}.json does not describe itself; skipped."
                        ));
                        continue;
                    }
                    if sample.vector.is_empty() {
                        continue;
                    }
                    if person != person_dir {
                        if samples
                            .iter()
                            .any(|s| s.model == model && s.person == person && s.clip == stem)
                        {
                            continue;
                        }
                        rehomed.insert(
                            (model.clone(), person.clone(), stem.clone()),
                            format!("{dir}/{stem}.json"),
                        );
                        sample.person.clone_from(&person);
                    }
                    samples.push(sample);
                }
            }
        }

        let mut terms: Vec<DictionaryTerm> =
            read_json_dir(root, DICTIONARY_DIR, &mut warnings, &mut newer)
                .into_iter()
                .filter_map(|(stem, term): (String, DictionaryTerm)| {
                    keep_if_named(stem, term, |t| &t.id, DICTIONARY_DIR, &mut warnings)
                })
                .collect();
        terms.sort_by(|a, b| {
            fold(&a.text)
                .cmp(&fold(&b.text))
                .then_with(|| a.id.cmp(&b.id))
        });

        Self {
            root: root.to_path_buf(),
            people,
            samples,
            clips,
            terms,
            tombstones,
            warnings,
            self_claims,
            newer,
            rehomed,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn person(&self, id: &str) -> Option<&Person> {
        self.people.iter().find(|person| person.id == id)
    }

    /// The person marked as the one using this bank.
    pub fn self_person(&self) -> Option<&Person> {
        self.people.iter().find(|person| person.is_self)
    }

    fn live_person(&self, id: &str) -> Result<&Person, BankError> {
        self.person(id)
            .ok_or_else(|| BankError::UnknownPerson(id.to_owned()))
    }

    /// Each person's voice for `model`: the L2-normalized mean of their
    /// L2-normalized vectors. People without a usable vector are absent.
    pub fn centroids(&self, model: &str) -> Vec<(String, Vec<f32>)> {
        let mut sums: HashMap<&str, (Vec<f32>, usize)> = HashMap::new();
        for sample in self.samples.iter().filter(|sample| sample.model == model) {
            let Some(unit) = normalized(&sample.vector) else {
                continue;
            };
            let entry = sums
                .entry(sample.person.as_str())
                .or_insert_with(|| (vec![0.0; unit.len()], 0));
            if entry.0.len() != unit.len() {
                continue;
            }
            for (sum, value) in entry.0.iter_mut().zip(&unit) {
                *sum += value;
            }
            entry.1 += 1;
        }
        self.people
            .iter()
            .filter_map(|person| {
                let (sum, _) = sums.get(person.id.as_str())?;
                Some((person.id.clone(), normalized(sum)?))
            })
            .collect()
    }

    /// Match one speaker embedding against every person's centroid (AD-346).
    pub fn match_speaker(&self, embedding: &[f32], model: &str) -> MatchResult {
        let unknown = MatchResult {
            status: MatchStatus::Unknown,
            person_id: None,
            name: None,
            score: None,
            candidates: Vec::new(),
        };
        let Some(query) = normalized(embedding) else {
            return unknown;
        };
        let mut scored: Vec<(f32, &Person)> = self
            .centroids(model)
            .into_iter()
            .filter(|(_, centroid)| centroid.len() == query.len())
            .filter_map(|(id, centroid)| Some((dot(&query, &centroid), self.person(&id)?)))
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
        let Some(&(best, best_person)) = scored.first() else {
            return unknown;
        };
        let candidate = |(score, person): &(f32, &Person)| Candidate {
            person_id: person.id.clone(),
            name: person.name.clone(),
            score: *score,
        };
        if best >= AUTO_MATCH {
            MatchResult {
                status: MatchStatus::Auto,
                person_id: Some(best_person.id.clone()),
                name: Some(best_person.name.clone()),
                score: Some(best),
                candidates: scored
                    .iter()
                    .skip(1)
                    .take_while(|(score, _)| *score >= SUGGEST)
                    .take(MAX_CANDIDATES)
                    .map(candidate)
                    .collect(),
            }
        } else if best >= SUGGEST {
            MatchResult {
                status: MatchStatus::Suggested,
                person_id: None,
                name: None,
                score: Some(best),
                candidates: scored
                    .iter()
                    .take_while(|(score, _)| *score >= SUGGEST)
                    .take(MAX_CANDIDATES)
                    .map(candidate)
                    .collect(),
            }
        } else {
            MatchResult {
                score: Some(best),
                ..unknown
            }
        }
    }

    /// A new person.
    pub fn create_person(&self, name: &str) -> Result<(Person, BankPlan), BankError> {
        let name = clean_name(name)?;
        let now = now_stamp();
        let person = Person {
            version: BANK_FILE_VERSION,
            id: ulid::Ulid::new().to_string(),
            name,
            aliases: Vec::new(),
            is_self: false,
            created_at: now.clone(),
            updated_at: now,
        };
        let plan = BankPlan {
            writes: vec![person_write(&person)?],
            deletes: Vec::new(),
        };
        Ok((person, plan))
    }

    pub fn rename_person(&self, id: &str, name: &str) -> Result<BankPlan, BankError> {
        let mut person = self.live_person(id)?.clone();
        person.name = clean_name(name)?;
        person.updated_at = now_stamp();
        self.guarded(BankPlan {
            writes: vec![person_write(&person)?],
            deletes: Vec::new(),
        })
    }

    /// Mark `id` as the person using the bank; everyone else who claims it
    /// (on disk, not only after the one-self rule) is unmarked.
    pub fn set_self(&self, id: &str) -> Result<BankPlan, BankError> {
        let mut writes = Vec::new();
        let now = now_stamp();
        let mut target = self.live_person(id)?.clone();
        target.is_self = true;
        target.updated_at.clone_from(&now);
        writes.push(person_write(&target)?);
        for claimant in self.self_claims.iter().filter(|claimant| *claimant != id) {
            if let Some(person) = self.person(claimant) {
                let mut person = person.clone();
                person.is_self = false;
                person.updated_at.clone_from(&now);
                writes.push(person_write(&person)?);
            }
        }
        self.guarded(BankPlan {
            writes,
            deletes: Vec::new(),
        })
    }

    /// Forget a person: a tombstone, and their person, clip and embedding files
    /// removed.
    pub fn delete_person(&self, id: &str) -> Result<BankPlan, BankError> {
        let person = self.live_person(id)?;
        let mut plan = BankPlan {
            writes: vec![tombstone_write(&person.id, None)?],
            deletes: vec![BankDelete {
                rel_path: person_path(&person.id),
            }],
        };
        for clip in self.clips.iter().filter(|clip| clip.person == person.id) {
            plan.deletes
                .extend(record_file(clip).map(|rel_path| BankDelete { rel_path }));
            plan.deletes.push(BankDelete {
                rel_path: clip.rel_path.clone(),
            });
        }
        plan.deletes.extend(
            self.samples
                .iter()
                .filter(|sample| sample.person == person.id)
                .map(|sample| BankDelete {
                    rel_path: self.sample_file(sample),
                }),
        );
        self.guarded(plan)
    }

    /// Two people are one: `from`'s clips and embeddings move to `into`, its
    /// name becomes an alias, and `from` is tombstoned as merged into `into`
    /// — so what another device still files under `from` counts as `into`'s.
    pub fn merge_people(&self, from: &str, into: &str) -> Result<BankPlan, BankError> {
        if from == into {
            return Err(BankError::SamePerson);
        }
        let source = self.live_person(from)?;
        let mut target = self.live_person(into)?.clone();
        let mut plan = BankPlan::default();
        let mut moved: HashSet<&str> = HashSet::new();
        for clip in self.clips.iter().filter(|clip| clip.person == source.id) {
            let bytes =
                std::fs::read(self.root.join(&clip.rel_path)).map_err(|error| BankError::Io {
                    path: clip.rel_path.clone(),
                    message: error.to_string(),
                })?;
            self.plan_move_clip(&mut plan, &source.id, &clip.clip, &target.id, Some(bytes))?;
            moved.insert(&clip.clip);
        }
        for sample in self
            .samples
            .iter()
            .filter(|sample| sample.person == source.id)
        {
            if moved.insert(&sample.clip) {
                self.plan_move_clip(&mut plan, &source.id, &sample.clip, &target.id, None)?;
            }
        }
        for alias in std::iter::once(&source.name).chain(&source.aliases) {
            let known = same_folded(&target.name, alias)
                || target
                    .aliases
                    .iter()
                    .any(|existing| same_folded(existing, alias));
            if !known {
                target.aliases.push(alias.clone());
            }
        }
        target.is_self |= source.is_self;
        target.updated_at = now_stamp();
        plan.writes.push(person_write(&target)?);
        plan.writes
            .push(tombstone_write(&source.id, Some(&target.id))?);
        plan.deletes.push(BankDelete {
            rel_path: person_path(&source.id),
        });
        self.guarded(plan)
    }

    /// Store a confirmed voice: the clip and its embedding (AD-347). Returns
    /// the clip id.
    ///
    /// A span already in the bank (the same transcript, start and end within
    /// [`SOURCE_TOLERANCE_S`]) is not stored twice: under the same person the
    /// plan only adds this model's embedding when it is missing — empty when
    /// it is not — and under another person the clip and its embeddings move
    /// to this one, which is how a wrong confirmation is corrected.
    pub fn add_sample(
        &self,
        person_id: &str,
        clip_wav_bytes: Vec<u8>,
        vector: Vec<f32>,
        model: &str,
        source: SampleSource,
    ) -> Result<(String, BankPlan), BankError> {
        let person = self.live_person(person_id)?;
        if !is_plain_segment(model) {
            return Err(BankError::InvalidModel(model.to_owned()));
        }
        if vector.is_empty() || normalized(&vector).is_none() {
            return Err(BankError::EmptySample);
        }
        let (clip, mut plan) = match self.heard_before(&source) {
            Some((owner, clip)) if owner == person.id => {
                if self
                    .samples
                    .iter()
                    .any(|s| s.model == model && s.person == owner && s.clip == clip)
                {
                    return Ok((clip, BankPlan::default()));
                }
                (clip, BankPlan::default())
            }
            Some((owner, clip)) => {
                let mut plan = BankPlan::default();
                self.plan_move_clip(&mut plan, &owner, &clip, &person.id, Some(clip_wav_bytes))?;
                let replaced = sample_path(model, &person.id, &clip);
                plan.writes.retain(|write| write.rel_path != replaced);
                (clip, plan)
            }
            None => {
                let clip = ulid::Ulid::new().to_string();
                let plan = BankPlan {
                    writes: vec![BankWrite {
                        rel_path: clip_path(&person.id, &clip),
                        bytes: clip_wav_bytes,
                    }],
                    deletes: Vec::new(),
                };
                (clip, plan)
            }
        };
        plan.writes.push(sample_write(&EmbeddingSample {
            version: BANK_FILE_VERSION,
            model: model.to_owned(),
            person: person.id.clone(),
            clip: clip.clone(),
            vector,
            source,
            added_at: now_stamp(),
        })?);
        Ok((clip, self.guarded(plan)?))
    }

    /// Store a confirmed voice's clip without an embedding — when none can be
    /// had now (the engine is busy); [`Self::missing_embeddings`] lists it for
    /// the next run to embed. Where it was heard is kept beside it
    /// ([`ClipRecord`]), and the same span is deduplicated as in
    /// [`Self::add_sample`]: already this person's is an empty plan, another
    /// person's moves to this one.
    pub fn add_clip_only(
        &self,
        person_id: &str,
        clip_wav_bytes: Vec<u8>,
        source: SampleSource,
    ) -> Result<(String, BankPlan), BankError> {
        let person = self.live_person(person_id)?;
        match self.heard_before(&source) {
            Some((owner, clip)) if owner == person.id => Ok((clip, BankPlan::default())),
            Some((owner, clip)) => {
                let mut plan = BankPlan::default();
                self.plan_move_clip(&mut plan, &owner, &clip, &person.id, Some(clip_wav_bytes))?;
                Ok((clip, self.guarded(plan)?))
            }
            None => {
                let clip = ulid::Ulid::new().to_string();
                let record = ClipRecord {
                    version: BANK_FILE_VERSION,
                    person: person.id.clone(),
                    clip: clip.clone(),
                    source,
                };
                let plan = BankPlan {
                    writes: vec![
                        BankWrite {
                            rel_path: clip_path(&person.id, &clip),
                            bytes: clip_wav_bytes,
                        },
                        clip_record_write(&record)?,
                    ],
                    deletes: Vec::new(),
                };
                Ok((clip, self.guarded(plan)?))
            }
        }
    }

    /// The one bank edit a confirmation makes (AD-347): the person (made,
    /// when new) and the speaker's voice sample, planned together so they
    /// land together. A sample the bank refuses is left out, with the reason
    /// in [`Confirmation::left_out`]; the person still lands. Confirming the
    /// microphone's own speaker (`ME`) says who the person recording is, so
    /// that person becomes self in the same plan — every other claimant
    /// unmarked — whoever was self before: re-confirming ME as someone else
    /// is how a wrong self is corrected.
    pub fn plan_confirmation(
        mut self,
        naming: Naming,
        sample: Option<VoiceSample>,
        model: &str,
        me: bool,
    ) -> Result<Confirmation, BankError> {
        let (person, mut plan) = match naming {
            Naming::New(name) => {
                let (person, plan) = self.create_person(&name)?;
                // The sample is planned for the person before its file exists.
                self.people.push(person.clone());
                (person, plan)
            }
            Naming::Existing(id) => (self.live_person(&id)?.clone(), BankPlan::default()),
        };
        let mut left_out = Vec::new();
        if let Some(sample) = sample {
            let planned = match sample.embedding {
                Some(vector) => {
                    self.add_sample(&person.id, sample.wav, vector, model, sample.source)
                }
                None => self.add_clip_only(&person.id, sample.wav, sample.source),
            };
            match planned {
                Ok((_, sample_plan)) => plan.extend(sample_plan),
                Err(error) => left_out.push(format!("the voice sample was not stored: {error}")),
            }
        }
        if me {
            match self.set_self(&person.id) {
                Ok(self_plan) => plan.extend(self_plan),
                Err(error) => left_out.push(format!("the person was not marked as you: {error}")),
            }
        }
        Ok(Confirmation {
            person,
            plan,
            left_out,
        })
    }

    /// The embedding of an existing clip under `model` — how a model change
    /// re-embeds the bank from its clips. Deterministic but for the vector:
    /// the source span comes from any other model's sample of the clip (or
    /// its [`ClipRecord`]), and `addedAt` is the clip's earliest sample's, else
    /// the time in the clip's ULID — so two devices re-embedding one clip plan
    /// the same file.
    pub fn plan_embedding(
        &self,
        person_id: &str,
        clip_id: &str,
        vector: Vec<f32>,
        model: &str,
    ) -> Result<BankPlan, BankError> {
        let person = self.live_person(person_id)?;
        if !is_plain_segment(model) {
            return Err(BankError::InvalidModel(model.to_owned()));
        }
        if normalized(&vector).is_none() {
            return Err(BankError::EmptySample);
        }
        let siblings = || {
            self.samples
                .iter()
                .filter(|sample| sample.person == person.id && sample.clip == clip_id)
        };
        let source = siblings()
            .map(|sample| sample.source.clone())
            .next()
            .or_else(|| {
                self.clips
                    .iter()
                    .find(|clip| clip.person == person.id && clip.clip == clip_id)
                    .and_then(|clip| clip.source.clone())
            })
            .unwrap_or_default();
        let added_at = siblings()
            .map(|sample| sample.added_at.as_str())
            .filter(|added_at| !added_at.is_empty())
            .min()
            .map_or_else(|| ulid_stamp(clip_id), str::to_owned);
        let sample = EmbeddingSample {
            version: BANK_FILE_VERSION,
            model: model.to_owned(),
            person: person.id.clone(),
            clip: clip_id.to_owned(),
            vector,
            source,
            added_at,
        };
        self.guarded(BankPlan {
            writes: vec![sample_write(&sample)?],
            deletes: Vec::new(),
        })
    }

    /// Clips with no embedding for `model`: `(person, clip, clip path)`. A
    /// clip whose embedding a newer keeper wrote is not missing.
    pub fn missing_embeddings(&self, model: &str) -> Vec<(String, String, String)> {
        let embedded: HashSet<(&str, &str)> = self
            .samples
            .iter()
            .filter(|sample| sample.model == model)
            .map(|sample| (sample.person.as_str(), sample.clip.as_str()))
            .collect();
        self.clips
            .iter()
            .filter(|clip| !embedded.contains(&(clip.person.as_str(), clip.clip.as_str())))
            .filter(|clip| {
                !self
                    .newer
                    .contains(&sample_path(model, &clip.person, &clip.clip))
            })
            .map(|clip| {
                (
                    clip.person.clone(),
                    clip.clip.clone(),
                    clip.rel_path.clone(),
                )
            })
            .collect()
    }

    /// How many clips a person has, and whether any has a `model` embedding.
    pub fn sample_facts(&self, person_id: &str, model: &str) -> (u32, bool) {
        let clips = self
            .clips
            .iter()
            .filter(|clip| clip.person == person_id)
            .count();
        let embedded = self
            .samples
            .iter()
            .any(|sample| sample.person == person_id && sample.model == model);
        (u32::try_from(clips).unwrap_or(u32::MAX), embedded)
    }

    /// The person and clip already holding `source`'s span, in any model or
    /// as a clip awaiting its embedding. A sample with no transcript says
    /// nothing about where it was heard and matches nothing.
    fn heard_before(&self, source: &SampleSource) -> Option<(String, String)> {
        if source.transcript.is_empty() {
            return None;
        }
        let same = |other: &SampleSource| {
            other.transcript == source.transcript
                && (other.start - source.start).abs() <= SOURCE_TOLERANCE_S
                && (other.end - source.end).abs() <= SOURCE_TOLERANCE_S
        };
        self.samples
            .iter()
            .find(|sample| same(&sample.source))
            .map(|sample| (sample.person.clone(), sample.clip.clone()))
            .or_else(|| {
                self.clips
                    .iter()
                    .find(|clip| clip.source.as_ref().is_some_and(same))
                    .map(|clip| (clip.person.clone(), clip.clip.clone()))
            })
    }

    /// Add to `plan` moving clip `clip` from `from` to `to`: its WAV (`bytes`,
    /// when there is one to write), its record and every model's embedding
    /// written under `to`, then the old files deleted.
    fn plan_move_clip(
        &self,
        plan: &mut BankPlan,
        from: &str,
        clip: &str,
        to: &str,
        bytes: Option<Vec<u8>>,
    ) -> Result<(), BankError> {
        if let Some(bytes) = bytes {
            plan.writes.push(BankWrite {
                rel_path: clip_path(to, clip),
                bytes,
            });
        }
        if let Some(file) = self
            .clips
            .iter()
            .find(|file| file.person == from && file.clip == clip)
        {
            if let (Some(source), Some(record)) = (&file.source, record_file(file)) {
                plan.writes.push(clip_record_write(&ClipRecord {
                    version: BANK_FILE_VERSION,
                    person: to.to_owned(),
                    clip: clip.to_owned(),
                    source: source.clone(),
                })?);
                plan.deletes.push(BankDelete { rel_path: record });
            }
            plan.deletes.push(BankDelete {
                rel_path: file.rel_path.clone(),
            });
        }
        for sample in self
            .samples
            .iter()
            .filter(|sample| sample.person == from && sample.clip == clip)
        {
            plan.writes.push(sample_write(&EmbeddingSample {
                person: to.to_owned(),
                ..sample.clone()
            })?);
            plan.deletes.push(BankDelete {
                rel_path: self.sample_file(sample),
            });
        }
        Ok(())
    }

    /// The file a loaded sample lives in: its own path, or — re-homed onto a
    /// merge survivor — the one still under the merged-away id.
    fn sample_file(&self, sample: &EmbeddingSample) -> String {
        self.rehomed
            .get(&(
                sample.model.clone(),
                sample.person.clone(),
                sample.clip.clone(),
            ))
            .cloned()
            .unwrap_or_else(|| sample_path(&sample.model, &sample.person, &sample.clip))
    }

    /// `plan`, unless it writes or deletes a file a newer keeper wrote.
    fn guarded(&self, plan: BankPlan) -> Result<BankPlan, BankError> {
        let touched = plan
            .writes
            .iter()
            .map(|write| &write.rel_path)
            .chain(plan.deletes.iter().map(|delete| &delete.rel_path));
        for path in touched {
            if self.newer.contains(path) {
                return Err(BankError::NewerFile(path.clone()));
            }
        }
        Ok(plan)
    }
}

pub(crate) fn person_path(id: &str) -> String {
    format!("{PEOPLE_DIR}/{id}.json")
}

pub(crate) fn clip_path(person: &str, clip: &str) -> String {
    format!("{CLIPS_DIR}/{person}/{clip}.wav")
}

pub(crate) fn sample_path(model: &str, person: &str, clip: &str) -> String {
    format!("{EMBEDDINGS_DIR}/{model}/{person}/{clip}.json")
}

pub(crate) fn term_path(id: &str) -> String {
    format!("{DICTIONARY_DIR}/{id}.json")
}

fn tombstone_path(id: &str) -> String {
    format!("{TOMBSTONES_DIR}/{id}.json")
}

/// Pretty JSON with a trailing newline: bank files diff line by line.
pub(crate) fn json_bytes<T: Serialize>(value: &T, rel_path: &str) -> Result<Vec<u8>, BankError> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| BankError::Io {
        path: rel_path.to_owned(),
        message: error.to_string(),
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn person_write(person: &Person) -> Result<BankWrite, BankError> {
    let rel_path = person_path(&person.id);
    Ok(BankWrite {
        bytes: json_bytes(person, &rel_path)?,
        rel_path,
    })
}

fn sample_write(sample: &EmbeddingSample) -> Result<BankWrite, BankError> {
    let rel_path = sample_path(&sample.model, &sample.person, &sample.clip);
    Ok(BankWrite {
        bytes: json_bytes(sample, &rel_path)?,
        rel_path,
    })
}

fn tombstone_write(id: &str, merged_into: Option<&str>) -> Result<BankWrite, BankError> {
    let rel_path = tombstone_path(id);
    let tombstone = Tombstone {
        id: id.to_owned(),
        deleted_at: now_stamp(),
        merged_into: merged_into.map(str::to_owned),
    };
    Ok(BankWrite {
        bytes: json_bytes(&tombstone, &rel_path)?,
        rel_path,
    })
}

fn clip_record_write(record: &ClipRecord) -> Result<BankWrite, BankError> {
    let rel_path = format!("{CLIPS_DIR}/{}/{}.json", record.person, record.clip);
    Ok(BankWrite {
        bytes: json_bytes(record, &rel_path)?,
        rel_path,
    })
}

/// The [`ClipRecord`] beside a clip that has one: its WAV's path as `.json`.
fn record_file(clip: &ClipFile) -> Option<String> {
    clip.source.as_ref()?;
    clip.rel_path
        .strip_suffix(".wav")
        .map(|stem| format!("{stem}.json"))
}

/// The time a ULID was minted, as a bank timestamp; the Unix epoch for an id
/// that is not a ULID, so the answer is still the same on every device.
fn ulid_stamp(id: &str) -> String {
    let millis = ulid::Ulid::from_string(id)
        .ok()
        .and_then(|ulid| i64::try_from(ulid.timestamp_ms()).ok())
        .unwrap_or(0);
    chrono::DateTime::from_timestamp_millis(millis)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn clean_name(name: &str) -> Result<String, BankError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(BankError::EmptyName);
    }
    Ok(name.to_owned())
}

pub(crate) fn now_stamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// `v / |v|`, or `None` for a zero, empty or non-finite vector.
pub(crate) fn normalized(vector: &[f32]) -> Option<Vec<f32>> {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    (norm.is_finite() && norm > f32::EPSILON)
        .then(|| vector.iter().map(|value| value / norm).collect())
}

pub(crate) fn dot(left: &[f32], right: &[f32]) -> f32 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}

/// Keep a record only when its file is named after its id — anything else
/// (a hand copy, a conflict copy) would make a later delete miss it.
fn keep_if_named<T>(
    stem: String,
    value: T,
    id: impl Fn(&T) -> &String,
    dir: &str,
    warnings: &mut Vec<String>,
) -> Option<T> {
    if *id(&value) == stem && is_plain_segment(&stem) {
        Some(value)
    } else {
        warnings.push(format!("{dir}/{stem}.json names another id; skipped."));
        None
    }
}

/// Every `*.json` in `<root>/<rel_dir>`, parsed. A file whose `version` is
/// above [`BANK_FILE_VERSION`] is skipped with a warning and remembered in
/// `newer` (root-relative), so no planner writes over what it cannot read.
fn read_json_dir<T: DeserializeOwned>(
    root: &Path,
    rel_dir: &str,
    warnings: &mut Vec<String>,
    newer: &mut HashSet<String>,
) -> Vec<(String, T)> {
    let dir = root.join(rel_dir);
    list_files(&dir, "json", warnings)
        .into_iter()
        .filter_map(|(stem, file)| {
            let path = dir.join(&file);
            let parsed = std::fs::read(&path)
                .map_err(|error| error.to_string())
                .and_then(|bytes| {
                    serde_json::from_slice::<serde_json::Value>(&bytes)
                        .map_err(|error| error.to_string())
                });
            let value = match parsed {
                Ok(value) => value,
                Err(error) => {
                    warnings.push(format!("{} could not be read: {error}", path.display()));
                    return None;
                }
            };
            let version = value.get("version").and_then(serde_json::Value::as_u64);
            if version.is_some_and(|version| version > u64::from(BANK_FILE_VERSION)) {
                warnings.push(format!(
                    "{rel_dir}/{file} was written by a newer keeper; skipped."
                ));
                newer.insert(format!("{rel_dir}/{file}"));
                return None;
            }
            match serde_json::from_value::<T>(value) {
                Ok(value) => Some((stem, value)),
                Err(error) => {
                    warnings.push(format!("{} could not be read: {error}", path.display()));
                    None
                }
            }
        })
        .collect()
}

/// `(stem, file name)` of every `*.<extension>` file in `dir`, sorted. An
/// absent directory is empty, not a warning.
fn list_files(dir: &Path, extension: &str, warnings: &mut Vec<String>) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = read_entries(dir, warnings)
        .into_iter()
        .filter(|(_, is_dir)| !is_dir)
        .filter_map(|(name, _)| {
            let stem = name.strip_suffix(extension)?.strip_suffix('.')?;
            (!stem.starts_with('.')).then(|| (stem.to_owned(), name.clone()))
        })
        .collect();
    files.sort();
    files
}

fn list_dirs(dir: &Path, warnings: &mut Vec<String>) -> Vec<String> {
    let mut dirs: Vec<String> = read_entries(dir, warnings)
        .into_iter()
        .filter(|(name, is_dir)| *is_dir && is_plain_segment(name) && !name.starts_with('.'))
        .map(|(name, _)| name)
        .collect();
    dirs.sort();
    dirs
}

fn read_entries(dir: &Path, warnings: &mut Vec<String>) -> Vec<(String, bool)> {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let is_dir = entry.file_type().ok()?.is_dir();
                Some((name, is_dir))
            })
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => {
            warnings.push(format!("{} could not be listed: {error}", dir.display()));
            Vec::new()
        }
    }
}

/// 16 kHz mono PCM16 WAV bytes for `samples_16k` (clamped to [-1, 1]).
pub fn wav_bytes(samples_16k: &[f32]) -> Vec<u8> {
    const HEADER: usize = 44;
    let data_len = samples_16k.len() * 2;
    let data_u32 = u32::try_from(data_len).unwrap_or(u32::MAX - 36);
    let mut bytes = Vec::with_capacity(HEADER + data_len);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_u32).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&WAV_SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(WAV_SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    bytes.extend_from_slice(&2u16.to_le_bytes()); // block align
    bytes.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_u32.to_le_bytes());
    for sample in samples_16k {
        // Saturating float→int cast: the clamp keeps it in range.
        let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// The samples of a 16 kHz mono PCM16 WAV, as f32 in [-1, 1].
pub fn wav_samples(bytes: &[u8]) -> Result<Vec<f32>, BankError> {
    let bad = |why: &str| BankError::Wav(why.to_owned());
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(bad("not a RIFF/WAVE file"));
    }
    let mut position = 12;
    let mut format_ok = false;
    while let Some(header) = bytes.get(position..position + 8) {
        let id = &header[0..4];
        let size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let body_start = position + 8;
        let body = bytes
            .get(body_start..body_start.saturating_add(size))
            .ok_or_else(|| bad("a chunk runs past the end"))?;
        match id {
            b"fmt " => {
                let field = |at: usize| {
                    body.get(at..at + 2)
                        .map(|b| u16::from_le_bytes([b[0], b[1]]))
                };
                let rate = body
                    .get(4..8)
                    .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
                if field(0) != Some(1)
                    || field(2) != Some(1)
                    || rate != Some(WAV_SAMPLE_RATE)
                    || field(14) != Some(16)
                {
                    return Err(bad("not 16 kHz mono PCM16"));
                }
                format_ok = true;
            }
            b"data" => {
                if !format_ok {
                    return Err(bad("no format before the samples"));
                }
                return Ok(body
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| f32::from(i16::from_le_bytes(*pair)) / 32768.0)
                    .collect());
            }
            _ => {}
        }
        position = body_start + size + (size & 1);
    }
    Err(bad("no samples"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("keeper-bank-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    /// Execute a plan the way the shell does: writes, then deletes.
    pub(crate) fn apply(root: &Path, plan: &BankPlan) {
        for write in &plan.writes {
            let path = root.join(&write.rel_path);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, &write.bytes).expect("write");
        }
        for delete in &plan.deletes {
            std::fs::remove_file(root.join(&delete.rel_path)).expect("delete");
        }
    }

    pub(crate) fn person(id: &str, name: &str, is_self: bool, updated_at: &str) -> Person {
        Person {
            version: 1,
            id: id.to_owned(),
            name: name.to_owned(),
            aliases: Vec::new(),
            is_self,
            created_at: "2026-09-01T00:00:00Z".to_owned(),
            updated_at: updated_at.to_owned(),
        }
    }

    fn write_json<T: Serialize>(root: &Path, rel: &str, value: &T) {
        apply(
            root,
            &BankPlan {
                writes: vec![BankWrite {
                    rel_path: rel.to_owned(),
                    bytes: json_bytes(value, rel).expect("json"),
                }],
                deletes: Vec::new(),
            },
        );
    }

    fn sample(model: &str, person: &str, clip: &str, vector: Vec<f32>) -> EmbeddingSample {
        EmbeddingSample {
            version: 1,
            model: model.to_owned(),
            person: person.to_owned(),
            clip: clip.to_owned(),
            vector,
            source: SampleSource::default(),
            added_at: "2026-09-01T00:00:00Z".to_owned(),
        }
    }

    /// A person with one clip + one embedding, written to disk.
    fn seed(root: &Path, id: &str, name: &str, vector: Vec<f32>) {
        write_json(
            root,
            &person_path(id),
            &person(id, name, false, "2026-09-01T00:00:00Z"),
        );
        let clip = format!("{id}C");
        apply(
            root,
            &BankPlan {
                writes: vec![BankWrite {
                    rel_path: clip_path(id, &clip),
                    bytes: wav_bytes(&[0.1; 32]),
                }],
                deletes: Vec::new(),
            },
        );
        write_json(
            root,
            &sample_path("m1", id, &clip),
            &sample("m1", id, &clip, vector),
        );
    }

    /// A unit 2-d vector at cosine `cos` from `[1, 0]`.
    fn at_cosine(cos: f32) -> Vec<f32> {
        vec![cos, (1.0 - cos * cos).sqrt()]
    }

    #[test]
    fn matching_honours_both_thresholds_at_their_boundaries() {
        let root = scratch();
        seed(&root, "A", "Ada", vec![1.0, 0.0]);
        let bank = Bank::load(&root);
        let status = |cos: f32| bank.match_speaker(&at_cosine(cos), "m1").status;
        assert_eq!(status(AUTO_MATCH + 0.001), MatchStatus::Auto);
        assert_eq!(status(AUTO_MATCH - 0.001), MatchStatus::Suggested);
        assert_eq!(status(SUGGEST + 0.001), MatchStatus::Suggested);
        assert_eq!(status(SUGGEST - 0.001), MatchStatus::Unknown);

        let auto = bank.match_speaker(&at_cosine(0.9), "m1");
        assert_eq!(auto.person_id.as_deref(), Some("A"));
        assert_eq!(auto.name.as_deref(), Some("Ada"));
        let suggested = bank.match_speaker(&at_cosine(0.6), "m1");
        assert_eq!(suggested.person_id, None, "a suggestion assigns nobody");
        assert_eq!(suggested.candidates[0].person_id, "A");
        assert_eq!(
            bank.match_speaker(&at_cosine(0.9), "other-model").status,
            MatchStatus::Unknown,
            "vectors of another embedding model are never compared"
        );
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_centroid_is_the_normalized_mean_of_normalized_vectors() {
        let root = scratch();
        seed(&root, "A", "Ada", vec![10.0, 0.0]);
        write_json(
            &root,
            &sample_path("m1", "A", "X"),
            &sample("m1", "A", "X", vec![0.0, 0.5]),
        );
        let bank = Bank::load(&root);
        let centroids = bank.centroids("m1");
        let (_, centroid) = &centroids[0];
        let half = std::f32::consts::FRAC_1_SQRT_2;
        assert!((centroid[0] - half).abs() < 1e-6 && (centroid[1] - half).abs() < 1e-6);
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_tombstone_beats_a_person_file_that_synced_back() {
        let root = scratch();
        seed(&root, "A", "Ada", vec![1.0, 0.0]);
        seed(&root, "B", "Bo", vec![0.0, 1.0]);
        let bank = Bank::load(&root);
        apply(&root, &bank.delete_person("A").expect("delete"));
        assert!(!root.join(person_path("A")).exists());
        // A device that never heard of the deletion pushes Ada's files back.
        seed(&root, "A", "Ada", vec![1.0, 0.0]);
        let bank = Bank::load(&root);
        assert_eq!(bank.person("A"), None);
        assert!(bank.samples.iter().all(|sample| sample.person != "A"));
        assert!(bank.clips.iter().all(|clip| clip.person != "A"));
        assert_eq!(
            bank.match_speaker(&[1.0, 0.0], "m1").status,
            MatchStatus::Unknown
        );
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn merging_moves_every_clip_and_embedding_and_keeps_the_name_as_an_alias() {
        let root = scratch();
        seed(&root, "A", "Ada L.", vec![1.0, 0.0]);
        seed(&root, "B", "Ada Lovelace", vec![0.9, 0.1]);
        let bank = Bank::load(&root);
        apply(&root, &bank.merge_people("A", "B").expect("merge"));
        let bank = Bank::load(&root);
        assert_eq!(bank.person("A"), None);
        let merged = bank.person("B").expect("into survives");
        assert_eq!(merged.aliases, ["Ada L."]);
        let clips: Vec<&str> = bank.clips.iter().map(|clip| clip.clip.as_str()).collect();
        assert_eq!(clips, ["AC", "BC"]);
        assert!(bank.clips.iter().all(|clip| clip.person == "B"));
        assert_eq!(bank.samples.len(), 2);
        assert!(bank.samples.iter().all(|sample| sample.person == "B"));
        assert_eq!(bank.merge_people("B", "B"), Err(BankError::SamePerson));
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn only_one_person_is_ever_self() {
        let root = scratch();
        write_json(
            &root,
            &person_path("A"),
            &person("A", "Ada", true, "2026-09-02T00:00:00Z"),
        );
        write_json(
            &root,
            &person_path("B"),
            &person("B", "Bo", true, "2026-09-03T00:00:00Z"),
        );
        write_json(
            &root,
            &person_path("C"),
            &person("C", "Cy", false, "2026-09-01T00:00:00Z"),
        );
        let bank = Bank::load(&root);
        assert_eq!(
            bank.self_person().map(|p| p.id.as_str()),
            Some("B"),
            "latest claim wins"
        );
        assert_eq!(bank.people.iter().filter(|p| p.is_self).count(), 1);
        assert_eq!(bank.warnings.len(), 1);

        apply(&root, &bank.set_self("C").expect("set self"));
        let bank = Bank::load(&root);
        let selves: Vec<&str> = bank
            .people
            .iter()
            .filter(|p| p.is_self)
            .map(|p| p.id.as_str())
            .collect();
        assert_eq!(selves, ["C"], "every other claim on disk is withdrawn");
        assert!(bank.warnings.is_empty());
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_bad_file_is_a_warning_not_a_blind_bank() {
        let root = scratch();
        seed(&root, "A", "Ada", vec![1.0, 0.0]);
        apply(
            &root,
            &BankPlan {
                writes: vec![
                    BankWrite {
                        rel_path: person_path("Z"),
                        bytes: b"{ not json".to_vec(),
                    },
                    BankWrite {
                        rel_path: person_path("Y"),
                        bytes: json_bytes(&person("X", "X", false, ""), "p").expect("json"),
                    },
                ],
                deletes: Vec::new(),
            },
        );
        let bank = Bank::load(&root);
        assert_eq!(bank.people.len(), 1);
        assert_eq!(bank.warnings.len(), 2);
        assert_eq!(Bank::load(&root.join("absent")).warnings.len(), 0);
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_new_sample_is_found_again_and_a_model_change_lists_it_as_missing() {
        let root = scratch();
        let bank = Bank::load(&root);
        let (ada, plan) = bank.create_person("  Ada ").expect("create");
        assert_eq!(ada.name, "Ada");
        apply(&root, &plan);
        let bank = Bank::load(&root);
        let (clip, plan) = bank
            .add_sample(
                &ada.id,
                wav_bytes(&[0.0; 16]),
                vec![1.0, 0.0],
                "m1",
                SampleSource::default(),
            )
            .expect("sample");
        apply(&root, &plan);
        let bank = Bank::load(&root);
        assert_eq!(bank.sample_facts(&ada.id, "m1"), (1, true));
        assert!(bank.missing_embeddings("m1").is_empty());
        assert_eq!(
            bank.missing_embeddings("m2"),
            [(ada.id.clone(), clip.clone(), clip_path(&ada.id, &clip))]
        );
        assert_eq!(
            bank.add_sample(
                &ada.id,
                Vec::new(),
                vec![1.0],
                "../m",
                SampleSource::default()
            ),
            Err(BankError::InvalidModel("../m".to_owned()))
        );
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn wav_round_trips_and_refuses_other_formats() {
        let samples = [0.0, 0.5, -0.5, 1.0, -1.0, 0.25];
        let bytes = wav_bytes(&samples);
        assert_eq!(bytes.len(), 44 + samples.len() * 2);
        let back = wav_samples(&bytes).expect("decode");
        assert_eq!(back.len(), samples.len());
        for (a, b) in samples.iter().zip(&back) {
            assert!((a - b).abs() < 1.0 / 16_000.0, "{a} vs {b}");
        }
        let mut stereo = bytes.clone();
        stereo[22] = 2;
        assert!(wav_samples(&stereo).is_err());
        assert!(wav_samples(b"RIFF....WAVE").is_err());
    }

    fn heard(start: f64, end: f64) -> SampleSource {
        SampleSource {
            transcript: "2026/call.mov.transcript.json".to_owned(),
            start,
            end,
        }
    }

    #[test]
    fn a_span_is_banked_once_and_a_wrong_confirmation_moves_it() {
        let root = scratch();
        seed(&root, "A", "Ada", vec![0.0, 1.0]);
        seed(&root, "B", "Bo", vec![0.0, 1.0]);
        let bank = Bank::load(&root);
        let (clip, plan) = bank
            .add_sample(
                "A",
                wav_bytes(&[0.2; 16]),
                vec![1.0, 0.0],
                "m1",
                heard(1.0, 5.0),
            )
            .expect("sample");
        apply(&root, &plan);

        let bank = Bank::load(&root);
        let (again, plan) = bank
            .add_sample(
                "A",
                wav_bytes(&[0.2; 16]),
                vec![1.0, 0.0],
                "m1",
                heard(1.004, 4.996),
            )
            .expect("same span");
        assert_eq!((again.as_str(), plan), (clip.as_str(), BankPlan::default()));
        let (_, plan) = bank
            .add_sample(
                "A",
                wav_bytes(&[0.2; 16]),
                vec![1.0, 0.0],
                "m1",
                heard(1.02, 5.0),
            )
            .expect("another span");
        assert!(!plan.writes.is_empty(), "20 ms away is another span");

        // The speaker was Bo all along.
        let (moved, plan) = bank
            .add_sample(
                "B",
                wav_bytes(&[0.2; 16]),
                vec![1.0, 0.0],
                "m1",
                heard(1.0, 5.0),
            )
            .expect("reconfirm");
        assert_eq!(moved, clip);
        apply(&root, &plan);
        let bank = Bank::load(&root);
        let of = |person: &str| -> Vec<&str> {
            bank.samples
                .iter()
                .filter(|s| s.person == person)
                .map(|s| s.clip.as_str())
                .collect()
        };
        assert_eq!(of("A"), ["AC"], "Ada keeps only her own voice");
        assert_eq!(of("B").len(), 2);
        assert!(of("B").contains(&clip.as_str()));
        assert!(bank.clips.iter().any(|c| c.person == "B" && c.clip == clip));
        assert!(!bank.clips.iter().any(|c| c.person == "A" && c.clip == clip));
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_clip_kept_without_an_embedding_remembers_its_span() {
        let root = scratch();
        seed(&root, "A", "Ada", vec![0.0, 1.0]);
        seed(&root, "B", "Bo", vec![0.0, 1.0]);
        let bank = Bank::load(&root);
        let (clip, plan) = bank
            .add_clip_only("A", wav_bytes(&[0.3; 16]), heard(10.0, 14.0))
            .expect("clip only");
        apply(&root, &plan);
        let bank = Bank::load(&root);
        assert!(bank
            .missing_embeddings("m1")
            .iter()
            .any(|(person, missing, _)| person == "A" && *missing == clip));
        let (same, plan) = bank
            .add_clip_only("A", wav_bytes(&[0.3; 16]), heard(10.0, 14.0))
            .expect("again");
        assert_eq!((same, plan), (clip.clone(), BankPlan::default()));
        let embedded = bank
            .plan_embedding("A", &clip, vec![1.0, 0.0], "m1")
            .expect("embed");
        let sample: EmbeddingSample =
            serde_json::from_slice(&embedded.writes[0].bytes).expect("sample");
        assert_eq!(sample.source, heard(10.0, 14.0), "the span survives");

        let (moved, plan) = bank
            .add_clip_only("B", wav_bytes(&[0.3; 16]), heard(10.0, 14.0))
            .expect("it was Bo");
        assert_eq!(moved, clip);
        apply(&root, &plan);
        let bank = Bank::load(&root);
        let file = bank
            .clips
            .iter()
            .find(|c| c.clip == clip)
            .expect("still one clip");
        assert_eq!(
            (file.person.as_str(), file.source.clone()),
            ("B", Some(heard(10.0, 14.0)))
        );
        assert_eq!(bank.clips.iter().filter(|c| c.clip == clip).count(), 1);
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn re_embedding_a_clip_plans_the_same_bytes_on_every_device() {
        let root = scratch();
        seed(&root, "A", "Ada", vec![1.0, 0.0]);
        let clip = ulid::Ulid::new().to_string();
        apply(
            &root,
            &BankPlan {
                writes: vec![BankWrite {
                    rel_path: clip_path("A", &clip),
                    bytes: wav_bytes(&[0.1; 16]),
                }],
                deletes: Vec::new(),
            },
        );
        let bank = Bank::load(&root);
        let first = bank
            .plan_embedding("A", "AC", vec![0.5, 0.5], "m2")
            .expect("plan");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let second = Bank::load(&root)
            .plan_embedding("A", "AC", vec![0.5, 0.5], "m2")
            .expect("plan");
        assert_eq!(first, second, "no clock in the bytes");
        let sample: EmbeddingSample = serde_json::from_slice(&first.writes[0].bytes).expect("json");
        assert_eq!(
            sample.added_at, "2026-09-01T00:00:00Z",
            "the clip's first sample's"
        );

        let unembedded = bank
            .plan_embedding("A", &clip, vec![0.5, 0.5], "m2")
            .expect("plan");
        let sample: EmbeddingSample =
            serde_json::from_slice(&unembedded.writes[0].bytes).expect("json");
        assert_eq!(sample.added_at, ulid_stamp(&clip));
        assert!(sample.added_at.starts_with("20"), "{}", sample.added_at);
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_file_from_a_newer_keeper_is_skipped_and_never_rewritten() {
        let root = scratch();
        seed(&root, "A", "Ada", vec![1.0, 0.0]);
        let mut future = sample("m2", "A", "AC", vec![0.0, 1.0]);
        future.version = 2;
        write_json(&root, &sample_path("m2", "A", "AC"), &future);
        let mut newer_person = person("N", "Nia", false, "");
        newer_person.version = 2;
        write_json(&root, &person_path("N"), &newer_person);

        let bank = Bank::load(&root);
        assert_eq!(bank.person("N"), None);
        assert!(bank.samples.iter().all(|s| s.model != "m2"));
        assert_eq!(
            bank.warnings
                .iter()
                .filter(|w| w.contains("newer keeper"))
                .count(),
            2
        );
        assert!(
            bank.missing_embeddings("m2").is_empty(),
            "its embedding exists, only this build cannot read it"
        );
        assert_eq!(
            bank.plan_embedding("A", "AC", vec![1.0, 1.0], "m2"),
            Err(BankError::NewerFile(sample_path("m2", "A", "AC")))
        );
        assert!(matches!(
            bank.delete_person("A"),
            Ok(plan) if plan.deletes.iter().all(|d| d.rel_path != sample_path("m2", "A", "AC"))
        ));
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn what_arrives_under_a_merged_away_person_counts_for_the_survivor() {
        let root = scratch();
        seed(&root, "A", "Ada", vec![1.0, 0.0]);
        seed(&root, "B", "Bo", vec![1.0, 0.0]);
        apply(
            &root,
            &Bank::load(&root).merge_people("A", "B").expect("merge"),
        );
        let tombstone: Tombstone = serde_json::from_slice(
            &std::fs::read(root.join(tombstone_path("A"))).expect("tombstone"),
        )
        .expect("json");
        assert_eq!(tombstone.merged_into.as_deref(), Some("B"));

        // A device that had not heard of the merge confirms another Ada line.
        apply(
            &root,
            &BankPlan {
                writes: vec![BankWrite {
                    rel_path: clip_path("A", "LATE"),
                    bytes: wav_bytes(&[0.1; 16]),
                }],
                deletes: Vec::new(),
            },
        );
        write_json(
            &root,
            &sample_path("m1", "A", "LATE"),
            &sample("m1", "A", "LATE", vec![0.0, 1.0]),
        );
        let bank = Bank::load(&root);
        assert_eq!(bank.person("A"), None);
        assert_eq!(bank.sample_facts("B", "m1"), (3, true));
        assert!(bank
            .samples
            .iter()
            .any(|s| s.person == "B" && s.clip == "LATE"));
        let (_, centroid) = &bank.centroids("m1")[0];
        assert!(centroid[1] > 0.1, "the late sample moves the centroid");

        // Deleting the survivor removes the late files where they really are.
        let plan = bank.delete_person("B").expect("delete");
        assert!(plan
            .deletes
            .iter()
            .any(|d| d.rel_path == sample_path("m1", "A", "LATE")));
        assert!(plan
            .deletes
            .iter()
            .any(|d| d.rel_path == clip_path("A", "LATE")));
        apply(&root, &plan);
        assert!(Bank::load(&root).samples.is_empty());
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_loop_of_merges_owns_nothing() {
        let root = scratch();
        seed(&root, "X", "Xi", vec![1.0, 0.0]);
        for (id, into) in [("X", "Y"), ("Y", "X")] {
            write_json(
                &root,
                &tombstone_path(id),
                &Tombstone {
                    id: id.to_owned(),
                    deleted_at: String::new(),
                    merged_into: Some(into.to_owned()),
                },
            );
        }
        let bank = Bank::load(&root);
        assert!(bank.people.is_empty() && bank.samples.is_empty() && bank.clips.is_empty());
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    /// The field report: confirming ME as Kelly by mistake made her self;
    /// confirming ME as Tomasz afterwards must move self to him.
    #[test]
    fn confirming_me_moves_self_to_that_person_and_a_call_voice_never_does() {
        let root = scratch();
        let confirm = |naming: Naming, me: bool| {
            let confirmation = Bank::load(&root)
                .plan_confirmation(naming, None, "m1", me)
                .expect("plan");
            assert!(
                confirmation.left_out.is_empty(),
                "{:?}",
                confirmation.left_out
            );
            apply(&root, &confirmation.plan);
            confirmation.person
        };
        let selves = || -> Vec<String> {
            Bank::load(&root)
                .people
                .iter()
                .filter(|p| p.is_self)
                .map(|p| p.name.clone())
                .collect()
        };

        let kelly = confirm(Naming::New("Kelly".to_owned()), true);
        assert_eq!(selves(), ["Kelly"]);
        let tomasz = confirm(Naming::New("Tomasz".to_owned()), true);
        assert_eq!(selves(), ["Tomasz"], "self moved, Kelly unmarked");
        confirm(Naming::Existing(kelly.id.clone()), true);
        assert_eq!(selves(), ["Kelly"], "and moves again");
        confirm(Naming::Existing(tomasz.id.clone()), false);
        assert_eq!(selves(), ["Kelly"], "a voice from the call is not you");
        confirm(Naming::New("Bo".to_owned()), false);
        assert_eq!(selves(), ["Kelly"]);

        let sample = VoiceSample {
            wav: wav_bytes(&[0.1; 1600]),
            embedding: Some(vec![1.0, 0.0]),
            source: SampleSource {
                transcript: "t.json".to_owned(),
                start: 1.0,
                end: 4.0,
            },
        };
        let confirmation = Bank::load(&root)
            .plan_confirmation(Naming::New("Ana".to_owned()), Some(sample), "m1", false)
            .expect("plan");
        apply(&root, &confirmation.plan);
        let bank = Bank::load(&root);
        assert_eq!(bank.sample_facts(&confirmation.person.id, "m1"), (1, true));
        assert!(matches!(
            bank.plan_confirmation(Naming::Existing("01NOBODY".to_owned()), None, "m1", true),
            Err(BankError::UnknownPerson(_))
        ));
        std::fs::remove_dir_all(&root).expect("cleanup");
    }
}
