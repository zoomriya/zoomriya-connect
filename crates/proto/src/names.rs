//! Rules for box names, such as `maplehouse` in `maplehouse.zoomriya.app`.
//!
//! The box checks these in setup and the control plane checks them again
//! (docs/architecture.md, "Registering a box"). The control plane adds its own
//! list of words on top: see [`NameRules::with_extra_words`].

/// Shortest name.
pub const MIN_LEN: usize = 3;
/// Longest name. With `-share` and `_acme-challenge.` it still fits a DNS label.
pub const MAX_LEN: usize = 32;
/// The guests' door is the name plus this.
pub const SHARE_SUFFIX: &str = "-share";

/// Names our own servers or the zone itself need.
const RESERVED: &[&str] = &[
    "www",
    "mail",
    "email",
    "smtp",
    "imap",
    "ns",
    "ns1",
    "ns2",
    "ns3",
    "dns",
    "connect",
    "relay",
    "relays",
    "share",
    "api",
    "admin",
    "status",
    "help",
    "support",
    "root",
    "localhost",
    "local",
    "box",
    "home",
    "test",
    "example",
    "invalid",
    "wpad",
    "autoconfig",
    "autodiscover",
    "zoomriya",
];

/// Words a scam name would use for a real padlock. Matched anywhere in the
/// name, after undoing look-alike spellings (`l0gin`, `pay-pal`).
const BLOCKED_WORDS: &[&str] = &[
    "zoomriya", "bank", "login", "logon", "signin", "pay", "wallet", "account", "verify", "secure",
    "password", "billing", "invoice", "refund", "unlock", "recovery", "official", "helpdesk",
    "update",
];

/// Why a name was refused. `code()` is stable for programs; `message()` is
/// what Box Desktop shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameRefused {
    /// Shorter than [`MIN_LEN`] or longer than [`MAX_LEN`].
    Length,
    /// Something other than a–z, 0–9 and single hyphens inside.
    Characters,
    /// Ends in `-share`, which is for guests' doors.
    ShareSuffix,
    /// Kept for our own servers.
    Reserved,
    /// Looks like a bank, a sign-in page or a brand.
    Blocked,
}

impl NameRefused {
    /// A stable code.
    pub fn code(self) -> &'static str {
        match self {
            NameRefused::Length => "name_length",
            NameRefused::Characters => "name_characters",
            NameRefused::ShareSuffix => "name_share",
            NameRefused::Reserved => "name_reserved",
            NameRefused::Blocked => "name_blocked",
        }
    }

    /// Plain words for the family.
    pub fn message(self) -> &'static str {
        match self {
            NameRefused::Length => "Pick a name of 3 to 32 letters or numbers.",
            NameRefused::Characters => {
                "Use only small letters, numbers and hyphens, with a letter or number at each end."
            }
            NameRefused::ShareSuffix => {
                "Names can't end in \"-share\". That ending is kept for share links."
            }
            NameRefused::Reserved => "That name is kept for ZOOMRIYA. Pick another.",
            NameRefused::Blocked => "That name isn't allowed. Pick another.",
        }
    }
}

/// The name rules, with any extra words the control plane's operators add.
#[derive(Debug, Clone, Default)]
pub struct NameRules {
    extra: Vec<String>,
}

impl NameRules {
    /// The built-in rules.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds words refused anywhere in a name, such as brand names. Blank lines
    /// and lines starting with `#` are skipped.
    pub fn with_extra_words<'a>(mut self, words: impl IntoIterator<Item = &'a str>) -> Self {
        for w in words {
            let w = w.trim();
            if w.is_empty() || w.starts_with('#') {
                continue;
            }
            let folded = fold(&w.to_ascii_lowercase());
            if !folded.is_empty() {
                self.extra.push(folded);
            }
        }
        self
    }

    /// Checks a name as typed. Names are compared in lower case; the caller
    /// stores [`normalise`]'s result.
    pub fn check(&self, name: &str) -> Result<(), NameRefused> {
        let name = normalise(name);
        check_syntax(&name)?;
        if name.ends_with(SHARE_SUFFIX) {
            return Err(NameRefused::ShareSuffix);
        }
        if RESERVED.contains(&name.as_str()) {
            return Err(NameRefused::Reserved);
        }
        // `1` stands in for both `l` and `i`, so read the name both ways.
        let folded = fold(&name);
        let folded_i = folded.replace('l', "i");
        let hit = |w: &str| folded.contains(w) || folded_i.contains(w);
        if BLOCKED_WORDS.iter().any(|w| hit(w)) || self.extra.iter().any(|w| hit(w)) {
            return Err(NameRefused::Blocked);
        }
        Ok(())
    }
}

/// Lower case, no surrounding spaces.
pub fn normalise(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

fn check_syntax(name: &str) -> Result<(), NameRefused> {
    if name.len() < MIN_LEN || name.len() > MAX_LEN {
        return Err(NameRefused::Length);
    }
    let ok_chars = name
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    // No hyphen at either end, and never two together: that also keeps out
    // `xn--` names, which browsers show as other alphabets.
    if !ok_chars || name.starts_with('-') || name.ends_with('-') || name.contains("--") {
        return Err(NameRefused::Characters);
    }
    Ok(())
}

/// Undoes look-alike spellings so `pay-pa1` reads as `paypal`.
fn fold(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '-')
        .map(|c| match c {
            '0' => 'o',
            '1' => 'l',
            '3' => 'e',
            '4' => 'a',
            '5' => 's',
            '7' => 't',
            '8' => 'b',
            '9' => 'g',
            c => c,
        })
        .collect()
}

/// The guests' door for a box name.
pub fn share_name(name: &str) -> String {
    format!("{name}{SHARE_SUFFIX}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_names() {
        let r = NameRules::new();
        for n in ["maplehouse", "elm-street-12", "  OakLane ", "abc"] {
            assert_eq!(r.check(n), Ok(()), "{n}");
        }
    }

    #[test]
    fn refused_names() {
        let r = NameRules::new().with_extra_words(["# brands", "", "acme"]);
        let cases = [
            ("ab", NameRefused::Length),
            (&"a".repeat(33), NameRefused::Length),
            ("maple_house", NameRefused::Characters),
            ("-maple", NameRefused::Characters),
            ("maple-", NameRefused::Characters),
            ("xn--maple", NameRefused::Characters),
            ("mäple", NameRefused::Characters),
            ("maple-share", NameRefused::ShareSuffix),
            ("share", NameRefused::Reserved),
            ("relay", NameRefused::Reserved),
            ("mybank", NameRefused::Blocked),
            ("l0gin-here", NameRefused::Blocked),
            ("ba-nk", NameRefused::Blocked),
            ("zoomr1ya-help", NameRefused::Blocked),
            ("acme-photos", NameRefused::Blocked),
            ("4cme", NameRefused::Blocked),
        ];
        for (name, why) in cases {
            assert_eq!(r.check(name), Err(why), "{name}");
        }
        // The extra words are the operators' own: the built-in rules allow it.
        assert_eq!(NameRules::new().check("acme-photos"), Ok(()));
    }

    #[test]
    fn longest_name_fits_dns() {
        let longest = "a".repeat(MAX_LEN);
        assert!(share_name(&longest).len() <= 63);
    }
}
