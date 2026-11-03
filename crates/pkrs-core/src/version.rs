//! Semantic versions and version requirements (§2.5, §9.7).
//!
//! `Version` and `VersionReq` are engine-owned newtypes over the `semver` crate; this module is
//! the only place that imports it (§4.5), so no `semver` type appears in a public signature.
//! Requirements use Cargo's flavour: `"0.3"` means `^0.3`, i.e. `>=0.3.0, <0.4.0`.

use core::fmt;
use core::str::FromStr;

use crate::error::{CoreError, Result};

/// A semantic version such as `0.3.1` (engine, game or plugin version).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(semver::Version);

impl Version {
    /// Creates a release version `major.minor.patch`.
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self(semver::Version::new(major, minor, patch))
    }

    /// Parses a full `major.minor.patch[-pre][+build]` version.
    pub fn parse(raw: &str) -> Result<Self> {
        semver::Version::parse(raw.trim())
            .map(Self)
            .map_err(|e| CoreError::InvalidVersion {
                input: raw.to_owned(),
                reason: e.to_string(),
            })
    }

    /// Major component.
    pub fn major(&self) -> u64 {
        self.0.major
    }

    /// Minor component.
    pub fn minor(&self) -> u64 {
        self.0.minor
    }

    /// Patch component.
    pub fn patch(&self) -> u64 {
        self.0.patch
    }
}

impl FromStr for Version {
    type Err = CoreError;

    fn from_str(raw: &str) -> Result<Self> {
        Self::parse(raw)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A semver requirement such as `"0.3"`, `"^1.2"` or `">=1.0, <2.0"`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct VersionReq(semver::VersionReq);

impl VersionReq {
    /// Parses a requirement (Cargo syntax; a bare version means `^version`).
    pub fn parse(raw: &str) -> Result<Self> {
        semver::VersionReq::parse(raw.trim())
            .map(Self)
            .map_err(|e| CoreError::InvalidVersionReq {
                input: raw.to_owned(),
                reason: e.to_string(),
            })
    }

    /// Returns `true` if `version` satisfies this requirement.
    pub fn matches(&self, version: &Version) -> bool {
        self.0.matches(&version.0)
    }
}

impl FromStr for VersionReq {
    type Err = CoreError;

    fn from_str(raw: &str) -> Result<Self> {
        Self::parse(raw)
    }
}

impl fmt::Display for VersionReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parses_and_orders() {
        let a = Version::parse("0.3.1").expect("valid");
        let b: Version = "0.10.0".parse().expect("valid");
        assert!(a < b);
        assert_eq!((a.major(), a.minor(), a.patch()), (0, 3, 1));
        assert_eq!(a.to_string(), "0.3.1");
        assert_eq!(Version::new(0, 3, 1), a);
    }

    #[test]
    fn version_rejects_partial_and_garbage() {
        assert!(Version::parse("0.3").is_err());
        assert!(Version::parse("one").is_err());
        let err = Version::parse("x").expect_err("invalid");
        assert!(err.to_string().starts_with("invalid version `x`"), "{err}");
    }

    #[test]
    fn bare_requirement_is_caret() {
        let req = VersionReq::parse("0.3").expect("valid");
        assert!(req.matches(&Version::new(0, 3, 0)));
        assert!(req.matches(&Version::new(0, 3, 9)));
        assert!(!req.matches(&Version::new(0, 4, 0)));
        assert!(!req.matches(&Version::new(0, 2, 9)));
    }

    #[test]
    fn caret_one_allows_minor_bumps() {
        let req: VersionReq = "^1.2".parse().expect("valid");
        assert!(req.matches(&Version::new(1, 9, 0)));
        assert!(!req.matches(&Version::new(2, 0, 0)));
    }

    #[test]
    fn compound_requirement() {
        let req = VersionReq::parse(">=1.0, <1.5").expect("valid");
        assert!(req.matches(&Version::new(1, 4, 9)));
        assert!(!req.matches(&Version::new(1, 5, 0)));
    }

    #[test]
    fn requirement_rejects_garbage() {
        let err = VersionReq::parse("latest").expect_err("invalid");
        assert!(
            err.to_string()
                .starts_with("invalid version requirement `latest`"),
            "{err}"
        );
    }
}
