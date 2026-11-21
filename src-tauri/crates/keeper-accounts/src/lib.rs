//! The accounts module's client half (story 114.5, DF/AD-2, DF/AD-6,
//! DF/AD-13): what a person's device does with each domain it holds given
//! the server's answer to `GET /databases` ([`plan`]), and the view of the
//! person's domains — each one's kind, name and who is in it ([`view`]).
//!
//! A person learns their own domains from discovery and who is in each from
//! that domain's own directory (`actors` on `dir:<domain>`), read through the
//! replica port; never from the map, which no device holds. A domain a
//! successful discovery no longer lists was left: the device stops serving
//! it and deletes its copy. A device the server removed keeps every copy
//! readable, as before.
//!
//! keeper-core's `Hq` owns the lifecycle, the paths, the authorization file
//! and the HTTP route (DW-E114-9) and asks this crate what to do; the shell
//! and `keeper-cli accounts domains` render [`view::DomainsVm`] as it is.

pub mod plan;
pub mod view;

pub use plan::{plan, Answer, Fate};
pub use view::{members, DomainMemberVm, DomainVm, DomainsState, DomainsVm, LeftDomainVm};
