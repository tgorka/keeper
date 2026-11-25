//! The reference judgement of a checked command against a job and its
//! claim: what the server's fences decide (122.2), written once so a client
//! and the server read every vector the same way. The server is the judge;
//! who may send which command (roles, kinds, ceilings) is its registry's,
//! not this module's.
//!
//! **Lapse.** Whether an attempt has lapsed is judged by the server's own
//! clock against the time the server itself recorded when it accepted the
//! attempt's last `jobs.claim` or `jobs.renew` (`renewed_at_ms`), never by
//! comparing one host's wall clock with another's: a claim is lapsed when
//! `now_ms >= renewed_at_ms + lease_ms`, both read on the server. A lapsed
//! claim only lets another client take the job over; until one does, the
//! attempt stays current. The fence is the `attempt_id`: every write of an
//! attempt names it, and once a new acquisition mints another, the old one's
//! writes are refused `stale_attempt`.
//!
//! **Epoch.** `epoch` counts acquisitions and rises on every one: a first
//! claim, a takeover, a re-claim after release, a restart with the same
//! client, and A→B→A. The session claim's rule (rise only on a new holder
//! client) is not this one.
//!
//! **Cancel and the attempt bound.** Cancelling a `queued` job ends it at once. Cancelling a
//! `claimed` one records `cancel_requested_at_ms` and lets the running attempt finish: its next
//! `jobs.renew` is refused `cancel_requested`, its `jobs.complete` is still recorded, and its
//! `jobs.release` or re-queueing `jobs.fail` ends the job `cancelled` (as does a takeover claim).
//! A job never outlives its attempt bound: releasing the last attempt ends it `dead`, and a claim
//! over a lapsed last attempt ends it `dead` while answering `attempts_exhausted`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::command::{AttemptRef, DeclareCapability, Payload};
use crate::model::{client_scope, AttemptId, Job, JobCapabilities, JobClaim, JobState};
use crate::{digest, page, Limits, Refusal};

/// Who sends the command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Caller {
    pub actor: String,
    pub client: String,
    #[serde(default)]
    pub context: Option<String>,
}

/// What the server knows when it judges a command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Context<'a> {
    /// The domain (database) the command was sent to.
    pub domain: &'a str,
    pub caller: &'a Caller,
    /// The server's clock.
    pub now_ms: u64,
    /// The attempt id the server minted for this command, used by an
    /// accepted `jobs.claim`.
    pub minted_attempt_id: Option<&'a AttemptId>,
    /// How many jobs of the domain have not ended.
    pub open_jobs: u64,
    pub limits: &'a Limits,
}

/// A job's two rows, before or after a command.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rows {
    #[serde(default)]
    pub job: Option<Job>,
    #[serde(default)]
    pub claim: Option<JobClaim>,
}

/// What a judged command writes. `code` is `None` when the command is accepted; `Some` when it is
/// refused yet its commit is applied, because the judgement ends the job (a claim beyond the
/// attempt bound or after a cancel request): the receipt is `rejected` with that code and the
/// commit's sequence.
#[derive(Clone, Debug, PartialEq)]
pub struct Judged {
    pub rows: Rows,
    pub code: Option<Refusal>,
}

/// Whether `claim` no longer holds its job at the server's `now_ms`: given
/// back, or not renewed within its lease by the server's own record.
#[must_use]
pub fn lapsed(claim: &JobClaim, now_ms: u64) -> bool {
    claim.released || now_ms >= claim.renewed_at_ms.saturating_add(claim.lease_ms)
}

/// Whether the command that took `before` to `after` must move the rows to
/// another `jobs_page` value (an explicit `rescope` in the same commit).
#[must_use]
pub fn moves(before: &Rows, after: &Rows) -> bool {
    match (&before.job, &after.job) {
        (Some(before), Some(after)) => before.page_key != after.page_key,
        _ => false,
    }
}

/// The input digest of a submission: the SHA-256 of the canonical input,
/// or of the input blob's reference, or of `null`.
pub fn input_digest(payload: &crate::command::Submit) -> Result<String, Refusal> {
    let value = match (&payload.input, &payload.input_blob) {
        (Some(input), _) => serde_json::to_value(input),
        (None, Some(blob)) => serde_json::to_value(blob),
        (None, None) => Ok(Value::Null),
    }
    .map_err(|_| Refusal::InvalidRequest)?;
    digest(&value)
}

/// Judges a checked job command against `before`. `jobs.declare_capability`
/// is [`declare`]'s.
pub fn judge(payload: &Payload, before: &Rows, ctx: &Context<'_>) -> Result<Judged, Refusal> {
    match payload {
        Payload::Submit(p) => submit(p, before, ctx).map(|rows| Judged { rows, code: None }),
        Payload::DeclareCapability(_) => Err(Refusal::InvalidRequest),
        Payload::Cancel(p) => {
            let job = open_job(before)?;
            if p.expected_revision != job.revision {
                return Err(Refusal::Conflict);
            }
            if job.state == JobState::Claimed {
                let mut job = job.clone();
                if job.cancel_requested_at_ms.is_none() {
                    job.cancel_requested_at_ms = Some(ctx.now_ms);
                }
                touch(&mut job, ctx);
                return Ok(Judged {
                    rows: Rows {
                        job: Some(job),
                        claim: before.claim.clone(),
                    },
                    code: None,
                });
            }
            Ok(Judged {
                rows: ended(before, job, JobState::Cancelled, ctx),
                code: None,
            })
        }
        Payload::Claim(p) => {
            let job = open_job(before)?;
            if ctx.now_ms < job.not_before_ms {
                return Err(Refusal::NotDue);
            }
            if p.expected_claim_revision != before.claim.as_ref().map_or(0, |c| c.revision) {
                return Err(Refusal::Conflict);
            }
            if let Some(claim) = &before.claim {
                let same_client = claim.holder_actor == ctx.caller.actor
                    && claim.holder_client == ctx.caller.client;
                if job.state == JobState::Claimed && !lapsed(claim, ctx.now_ms) && !same_client {
                    return Err(Refusal::Conflict);
                }
            }
            if job.cancel_requested_at_ms.is_some() {
                return Ok(Judged {
                    rows: ended(before, job, JobState::Cancelled, ctx),
                    code: Some(Refusal::CancelRequested),
                });
            }
            if job.attempt >= job.max_attempts {
                return Ok(Judged {
                    rows: ended(before, job, JobState::Dead, ctx),
                    code: Some(Refusal::AttemptsExhausted),
                });
            }
            let attempt_id = ctx
                .minted_attempt_id
                .ok_or(Refusal::InvalidRequest)?
                .clone();
            let (epoch, revision) = before
                .claim
                .as_ref()
                .map_or((1, 1), |c| (c.epoch + 1, c.revision + 1));
            let mut job = job.clone();
            job.state = JobState::Claimed;
            job.attempt += 1;
            touch(&mut job, ctx);
            let claim = JobClaim {
                job_id: job.id.clone(),
                domain: job.domain.clone(),
                page_key: job.page_key.clone(),
                holder_actor: ctx.caller.actor.clone(),
                holder_client: ctx.caller.client.clone(),
                attempt_id,
                epoch,
                lease_ms: p.lease_ms,
                acquired_at_ms: ctx.now_ms,
                renewed_at_ms: ctx.now_ms,
                released: false,
                revision,
                author_actor: ctx.caller.actor.clone(),
                author_client: ctx.caller.client.clone(),
            };
            Ok(Judged {
                rows: Rows {
                    job: Some(job),
                    claim: Some(claim),
                },
                code: None,
            })
        }
        Payload::Renew(p) => {
            let (job, claim) = current(before, p, ctx)?;
            if job.cancel_requested_at_ms.is_some() {
                return Err(Refusal::CancelRequested);
            }
            let mut claim = claim.clone();
            claim.renewed_at_ms = ctx.now_ms;
            claim.revision += 1;
            author(&mut claim, ctx);
            Ok(Judged {
                rows: Rows {
                    job: Some(job.clone()),
                    claim: Some(claim),
                },
                code: None,
            })
        }
        Payload::Release(p) => {
            let (job, claim) = current(before, p, ctx)?;
            let mut job = job.clone();
            if job.cancel_requested_at_ms.is_some() {
                end(&mut job, JobState::Cancelled, ctx);
            } else if job.attempt >= job.max_attempts {
                end(&mut job, JobState::Dead, ctx);
            } else {
                job.state = JobState::Queued;
                touch(&mut job, ctx);
            }
            let mut claim = claim.clone();
            release(&mut claim, &job, ctx);
            Ok(Judged {
                rows: Rows {
                    job: Some(job),
                    claim: Some(claim),
                },
                code: None,
            })
        }
        Payload::Progress(p) => {
            let (job, claim) = current(before, &attempt_ref(&p.job_id, &p.attempt_id), ctx)?;
            if p.input_digest != job.input_digest {
                return Err(Refusal::InputMoved);
            }
            let mut job = job.clone();
            if let Some(plan) = &p.plan_blob {
                if job.plan_blob.is_some() {
                    return Err(Refusal::PlanExists);
                }
                job.plan_blob = Some(plan.clone());
                job.plan_attempt_id = Some(p.attempt_id.clone());
            }
            if p.note.is_some() {
                job.note.clone_from(&p.note);
            }
            touch(&mut job, ctx);
            Ok(Judged {
                rows: Rows {
                    job: Some(job),
                    claim: Some(claim.clone()),
                },
                code: None,
            })
        }
        Payload::Complete(p) => {
            let (job, claim) = current(before, &attempt_ref(&p.job_id, &p.attempt_id), ctx)?;
            let mut job = job.clone();
            job.result.clone_from(&p.result);
            job.result_blob.clone_from(&p.result_blob);
            job.effects = Some(p.effects.clone());
            end(&mut job, JobState::Succeeded, ctx);
            let mut claim = claim.clone();
            release(&mut claim, &job, ctx);
            Ok(Judged {
                rows: Rows {
                    job: Some(job),
                    claim: Some(claim),
                },
                code: None,
            })
        }
        Payload::Fail(p) => {
            let (job, claim) = current(before, &attempt_ref(&p.job_id, &p.attempt_id), ctx)?;
            let mut job = job.clone();
            job.error_code = Some(p.error_code.clone());
            job.error_sentence = Some(p.sentence.clone());
            job.effects = Some(p.effects.clone());
            if p.retryable && job.attempt < job.max_attempts {
                if job.cancel_requested_at_ms.is_some() {
                    end(&mut job, JobState::Cancelled, ctx);
                } else {
                    job.state = JobState::Queued;
                    touch(&mut job, ctx);
                }
            } else {
                let state = if p.retryable {
                    JobState::Dead
                } else {
                    JobState::Failed
                };
                end(&mut job, state, ctx);
            }
            let mut claim = claim.clone();
            release(&mut claim, &job, ctx);
            Ok(Judged {
                rows: Rows {
                    job: Some(job),
                    claim: Some(claim),
                },
                code: None,
            })
        }
    }
}

/// Judges `jobs.declare_capability`: the caller's own row, replaced whole,
/// its capabilities ascending and distinct.
#[must_use]
pub fn declare(
    payload: &DeclareCapability,
    before: Option<&JobCapabilities>,
    ctx: &Context<'_>,
) -> JobCapabilities {
    let mut capabilities = payload.capabilities.clone();
    capabilities.sort();
    capabilities.dedup();
    JobCapabilities {
        id: client_scope(&ctx.caller.actor, &ctx.caller.client),
        domain: ctx.domain.to_owned(),
        actor_id: ctx.caller.actor.clone(),
        client_id: ctx.caller.client.clone(),
        capabilities,
        declared_at_ms: ctx.now_ms,
        revision: before.map_or(1, |row| row.revision + 1),
        author_actor: ctx.caller.actor.clone(),
        author_client: ctx.caller.client.clone(),
    }
}

fn submit(p: &crate::command::Submit, before: &Rows, ctx: &Context<'_>) -> Result<Rows, Refusal> {
    if before.job.is_some() {
        return Err(Refusal::JobExists);
    }
    if ctx.open_jobs >= ctx.limits.open_max_per_domain {
        return Err(Refusal::TooManyOpen);
    }
    let input = p
        .input
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|_| Refusal::InvalidRequest)?;
    let job = Job {
        id: p.job_id.clone(),
        domain: ctx.domain.to_owned(),
        page_key: page::open(ctx.domain),
        kind: p.kind.clone(),
        input,
        input_blob: p.input_blob.clone(),
        input_digest: input_digest(p)?,
        runs_on: p.runs_on.clone(),
        state: JobState::Queued,
        attempt: 0,
        max_attempts: p.max_attempts,
        not_before_ms: p.not_before_ms.unwrap_or(ctx.now_ms),
        plan_blob: None,
        plan_attempt_id: None,
        note: None,
        result: None,
        result_blob: None,
        effects: None,
        error_code: None,
        error_sentence: None,
        submitter_actor: ctx.caller.actor.clone(),
        submitter_client: ctx.caller.client.clone(),
        submitter_context: ctx.caller.context.clone(),
        submitted_at_ms: ctx.now_ms,
        updated_at_ms: ctx.now_ms,
        ended_at_ms: None,
        cancel_requested_at_ms: None,
        revision: 1,
        author_actor: ctx.caller.actor.clone(),
        author_client: ctx.caller.client.clone(),
    };
    Ok(Rows {
        job: Some(job),
        claim: None,
    })
}

fn attempt_ref(job_id: &str, attempt_id: &AttemptId) -> AttemptRef {
    AttemptRef {
        job_id: job_id.to_owned(),
        attempt_id: attempt_id.clone(),
    }
}

/// The job, refused when absent or ended.
fn open_job(before: &Rows) -> Result<&Job, Refusal> {
    let job = before.job.as_ref().ok_or(Refusal::UnknownJob)?;
    if job.state.is_terminal() {
        return Err(Refusal::Terminal);
    }
    Ok(job)
}

/// The job and its claim when `p` names the current attempt, held by the caller.
fn current<'a>(
    before: &'a Rows,
    p: &AttemptRef,
    ctx: &Context<'_>,
) -> Result<(&'a Job, &'a JobClaim), Refusal> {
    let job = open_job(before)?;
    let claim = before.claim.as_ref().ok_or(Refusal::StaleAttempt)?;
    if job.state != JobState::Claimed
        || claim.released
        || claim.attempt_id != p.attempt_id
        || claim.holder_actor != ctx.caller.actor
        || claim.holder_client != ctx.caller.client
    {
        return Err(Refusal::StaleAttempt);
    }
    Ok((job, claim))
}

fn touch(job: &mut Job, ctx: &Context<'_>) {
    job.updated_at_ms = ctx.now_ms;
    job.revision += 1;
    job.author_actor.clone_from(&ctx.caller.actor);
    job.author_client.clone_from(&ctx.caller.client);
}

fn author(claim: &mut JobClaim, ctx: &Context<'_>) {
    claim.author_actor.clone_from(&ctx.caller.actor);
    claim.author_client.clone_from(&ctx.caller.client);
}

/// Ends `job` in `state` and moves it to the month it ended in.
fn end(job: &mut Job, state: JobState, ctx: &Context<'_>) {
    job.state = state;
    job.ended_at_ms = Some(ctx.now_ms);
    job.page_key = page::month(&job.domain, ctx.now_ms);
    touch(job, ctx);
}

/// `before` with `job` ended in `state` and its claim, if any, given back and moved with it.
fn ended(before: &Rows, job: &Job, state: JobState, ctx: &Context<'_>) -> Rows {
    let mut job = job.clone();
    end(&mut job, state, ctx);
    let mut claim = before.claim.clone();
    if let Some(claim) = claim.as_mut() {
        release(claim, &job, ctx);
    }
    Rows {
        job: Some(job),
        claim,
    }
}

/// Gives the claim back and keeps it under its job's page.
fn release(claim: &mut JobClaim, job: &Job, ctx: &Context<'_>) {
    claim.released = true;
    claim.page_key.clone_from(&job.page_key);
    claim.revision += 1;
    author(claim, ctx);
}
