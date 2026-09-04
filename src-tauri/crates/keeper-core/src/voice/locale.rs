//! Which language the recogniser runs in (Epic 63, AD-175, NFR-11).
//!
//! On-device recognition exists for a few locales on any one device — four
//! of sixty-three on the Mac this was measured on, all English — and the
//! system locale is not necessarily among them. A port that followed the
//! system locale alone was therefore dead on a Polish-language device even
//! though English could have run there, and the sentence it showed told the
//! person that no download would help. Both were wrong, and both are fixed
//! by one pure function: [`choose`] takes what the person asked for
//! (`bots.voice_locale`, unset meaning "choose for me"), what the system is
//! set to, and which locales this device can run on its own, and answers the
//! locale to build the recogniser for or the refusal to show.
//!
//! # The rules
//!
//! - **An explicit request that can run on the device wins.** It is returned
//!   as the port spells it, so the recogniser is built from an identifier the
//!   framework itself listed.
//! - **An explicit request that cannot is a refusal, never a fallback.** A
//!   person who chose Polish and silently got English would be misled about
//!   what keeper is hearing; the refusal names the locales that can run
//!   instead, so the next choice is an informed one.
//! - **Unset prefers the system locale** when it can run on the device;
//!   otherwise a locale sharing the system's language (`en-GB` for an `en-US`
//!   system with no `en-US` model); otherwise the first of the port's list,
//!   which every port hands over sorted by identifier so the answer is the
//!   same on every launch. What was chosen is visible on the surface as the
//!   locale in force, so a fallback is never silent either.
//! - **Nothing can run on the device** is the same refusal with an empty list,
//!   and its sentence says that no language here runs locally rather than
//!   offering a choice of none.
//!
//! # Identifiers
//!
//! The system locale reads back as `en_US` — an underscore — while the
//! framework lists `en-US`. `SFSpeechRecognizer` accepts either, but a
//! membership test that compared the two forms byte for byte would answer
//! false for every locale on every device. So every comparison here goes
//! through [`same`], which maps `_` to `-` and ignores ASCII case, and every
//! identifier that reaches a sentence goes through [`canonical`], the hyphen
//! form the framework and the surface use.

use super::VoiceUnavailable;

/// What a port knows about locales on its device: the inputs to [`choose`]
/// that are the OS's rather than the person's.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeviceLocales {
    /// The system locale as the OS spells it (`en_US` on macOS).
    pub system: String,
    /// Every locale whose recogniser reports `supportsOnDeviceRecognition`,
    /// as the framework spells it (`en-US`), sorted by identifier so the
    /// first is the same on every launch. Empty when none can.
    pub on_device: Vec<String>,
}

/// `id` in the framework's spelling: `_` becomes `-`. Case is kept, because
/// `pl-PL` is how a person expects to read it and the framework's own
/// identifiers are already cased that way.
pub fn canonical(id: &str) -> String {
    id.trim().replace('_', "-")
}

/// Whether `a` and `b` name the same locale, whichever separator and case
/// each was written with.
pub fn same(a: &str, b: &str) -> bool {
    let mut a = a.trim().chars().map(fold);
    let mut b = b.trim().chars().map(fold);
    loop {
        match (a.next(), b.next()) {
            (None, None) => return true,
            (Some(x), Some(y)) if x == y => {}
            _ => return false,
        }
    }
}

/// The language subtag of `id` — `en` of `en_US` — for the same-language
/// preference here and for [`super::speech`], whose detected languages and
/// voice languages meet at the subtag. Lowercased, so `EN-us` and `en-GB`
/// share one.
pub fn language(id: &str) -> String {
    id.trim()
        .split(['_', '-'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn fold(c: char) -> char {
    match c {
        '_' => '-',
        other => other.to_ascii_lowercase(),
    }
}

/// The locale to run in, or the refusal.
///
/// `requested` is `bots.voice_locale`; `None` or blank means "choose for
/// me". `system` is the system locale in whichever spelling the OS gave it.
/// `on_device` is every locale whose recogniser reports
/// `supportsOnDeviceRecognition`, in the port's sorted order. The `Ok`
/// value is always an element of `on_device`, spelled as the port spelled
/// it. The `Err` is always [`VoiceUnavailable::NoOnDeviceRecognition`],
/// naming the locale that was asked for (or the system's, when nothing was)
/// in canonical form, and carrying `on_device` so its sentence can list the
/// alternatives.
pub fn choose(
    requested: Option<&str>,
    system: &str,
    on_device: &[String],
) -> Result<String, VoiceUnavailable> {
    decide(requested, system, on_device).map_err(VoiceUnavailable::from)
}

/// The locale in force, for the surface: what [`choose`] answered, or —
/// when it refused — the locale it refused, so a picker shows the language
/// the person chose beside the sentence saying it cannot run.
pub fn in_force(requested: Option<&str>, system: &str, on_device: &[String]) -> String {
    decide(requested, system, on_device).unwrap_or_else(|refused| refused.locale)
}

/// A locale that cannot run on this device, with what can.
struct Refused {
    locale: String,
    on_device: Vec<String>,
}

impl From<Refused> for VoiceUnavailable {
    fn from(refused: Refused) -> Self {
        VoiceUnavailable::NoOnDeviceRecognition {
            locale: refused.locale,
            on_device: refused.on_device,
        }
    }
}

fn decide(requested: Option<&str>, system: &str, on_device: &[String]) -> Result<String, Refused> {
    let find = |wanted: &str| on_device.iter().find(|have| same(have, wanted)).cloned();
    let refuse = |wanted: &str| Refused {
        locale: canonical(wanted),
        on_device: on_device.to_vec(),
    };
    match requested.map(str::trim).filter(|r| !r.is_empty()) {
        Some(wanted) => find(wanted).ok_or_else(|| refuse(wanted)),
        None => find(system)
            .or_else(|| {
                let tongue = language(system);
                on_device
                    .iter()
                    .find(|have| language(have) == tongue)
                    .cloned()
            })
            .or_else(|| on_device.first().cloned())
            .ok_or_else(|| refuse(system)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    /// The four locales hesperia can run, as `supportedLocales()` spells them.
    fn hesperia() -> Vec<String> {
        list(&["en-ID", "en-PH", "en-SA", "en-US"])
    }

    fn refused(result: Result<String, VoiceUnavailable>) -> (String, Vec<String>) {
        match result {
            Err(VoiceUnavailable::NoOnDeviceRecognition { locale, on_device }) => {
                (locale, on_device)
            }
            other => panic!("expected a NoOnDeviceRecognition refusal, got {other:?}"),
        }
    }

    #[test]
    fn identifiers_compare_across_separator_and_case() {
        assert!(same("en_US", "en-US"));
        assert!(same("EN-us", "en_US"));
        assert!(same(" pl-PL ", "pl_PL"));
        assert!(!same("en-US", "en-GB"));
        assert!(!same("en", "en-US"));
        assert_eq!(canonical("en_US"), "en-US");
        assert_eq!(canonical("pl-PL"), "pl-PL");
    }

    /// The live trap: the system locale reads `en_US`, the framework lists
    /// `en-US`. Unset must still find it, and answer in the framework's form.
    #[test]
    fn unset_takes_the_system_locale_across_the_underscore() {
        assert_eq!(
            choose(None, "en_US", &hesperia()).expect("a capable locale"),
            "en-US"
        );
        assert_eq!(in_force(None, "en_US", &hesperia()), "en-US");
    }

    #[test]
    fn blank_request_is_unset() {
        assert_eq!(
            choose(Some(""), "en_US", &hesperia()).expect("a capable locale"),
            "en-US"
        );
        assert_eq!(
            choose(Some("   "), "en_US", &hesperia()).expect("a capable locale"),
            "en-US"
        );
    }

    #[test]
    fn explicit_capable_request_wins_over_the_system_locale() {
        assert_eq!(
            choose(Some("en-PH"), "en_US", &hesperia()).expect("a capable locale"),
            "en-PH"
        );
        // Whatever spelling the setting holds, the answer is the port's.
        assert_eq!(
            choose(Some("en_ph"), "en_US", &hesperia()).expect("a capable locale"),
            "en-PH"
        );
        assert_eq!(in_force(Some("en_ph"), "en_US", &hesperia()), "en-PH");
    }

    /// The owner's case: a Polish phone, Polish asked for, only English runs.
    /// No silent English — a refusal that names what can run.
    #[test]
    fn explicit_incapable_request_is_refused_naming_the_capable_ones() {
        let (locale, on_device) = refused(choose(Some("pl_PL"), "pl_PL", &hesperia()));
        assert_eq!(locale, "pl-PL");
        assert_eq!(on_device, hesperia());
        // The locale in force is the one the person chose, not a fallback.
        assert_eq!(in_force(Some("pl_PL"), "pl_PL", &hesperia()), "pl-PL");
    }

    /// A request for something `supportedLocales()` never listed is the same
    /// refusal: the list of what can run is what matters, not why it cannot.
    #[test]
    fn request_outside_supported_locales_is_the_same_refusal() {
        let (locale, on_device) = refused(choose(Some("tlh-XX"), "en_US", &hesperia()));
        assert_eq!(locale, "tlh-XX");
        assert_eq!(on_device, hesperia());
    }

    /// Unset on a Polish system with only English models: the first of the
    /// port's sorted list, visibly, rather than a refusal.
    #[test]
    fn unset_with_an_incapable_system_locale_takes_the_first_capable() {
        assert_eq!(
            choose(None, "pl_PL", &hesperia()).expect("a capable locale"),
            "en-ID"
        );
        assert_eq!(in_force(None, "pl_PL", &hesperia()), "en-ID");
    }

    /// Unset, the system's exact locale cannot run but one of its language
    /// can: the same language beats the first of the list.
    #[test]
    fn unset_prefers_the_system_language_before_the_first_capable() {
        let models = list(&["de-DE", "en-GB", "en-US"]);
        assert_eq!(
            choose(None, "en_AU", &models).expect("a capable locale"),
            "en-GB"
        );
        assert_eq!(
            choose(None, "de_AT", &models).expect("a capable locale"),
            "de-DE"
        );
        assert_eq!(
            choose(None, "fr_FR", &models).expect("a capable locale"),
            "de-DE"
        );
    }

    #[test]
    fn empty_capable_list_is_a_refusal_with_nothing_to_offer() {
        let (locale, on_device) = refused(choose(None, "en_US", &[]));
        assert_eq!(locale, "en-US");
        assert!(on_device.is_empty());
        let (locale, on_device) = refused(choose(Some("pl-PL"), "en_US", &[]));
        assert_eq!(locale, "pl-PL");
        assert!(on_device.is_empty());
        assert_eq!(in_force(None, "en_US", &[]), "en-US");
    }

    /// Whatever the inputs, an `Ok` is a member of the list as spelled there.
    #[test]
    fn a_choice_is_always_one_of_the_capable_list() {
        let models = hesperia();
        for requested in [None, Some("en_US"), Some("EN-SA"), Some("en-id")] {
            for system in ["en_US", "pl_PL", "en-GB"] {
                let chosen = choose(requested, system, &models).expect("a capable locale");
                assert!(models.contains(&chosen), "{requested:?}/{system}: {chosen}");
            }
        }
    }
}
