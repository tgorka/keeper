//! A device's side of the settings that live with its data (epic 121).
//!
//! - [`models`]: the organisation's model set, published as `config_values`
//!   rows of the general domain (story 121.3), hydrated onto this device by
//!   path through the blob port's download.
//!
//! An implementation (DF/AD-2/3): it depends on the settings contract, the
//! ports and the path leaf only.

pub mod models;
