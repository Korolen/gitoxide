use bitflags::bitflags;
use bstr::{BStr, ByteSlice};

use crate::{pattern, wildmatch, Pattern};

bitflags! {
    /// Information about a [`Pattern`].
    ///
    /// Its main purpose is to accelerate pattern matching, or to negate the match result or to
    /// keep special rules only applicable when matching paths.
    ///
    /// The mode is typically created when parsing the pattern by inspecting it and isn't typically handled by the user.
    #[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
    pub struct Mode: u32 {
        /// The pattern does not contain a sub-directory and - it doesn't contain slashes after removing the trailing one.
        const NO_SUB_DIR = 1 << 0;
        /// A pattern that is '*literal', meaning that it ends with what's given here
        const ENDS_WITH = 1 << 1;
        /// The pattern must match a directory, and not a file.
        const MUST_BE_DIR = 1 << 2;
        /// The pattern matches, but should be negated. Note that this mode has to be checked and applied by the caller.
        const NEGATIVE = 1 << 3;
        /// The pattern starts with a slash and thus matches only from the beginning.
        const ABSOLUTE = 1 << 4;
    }
}

/// Describes whether to match a path case sensitively or not.
///
/// Used in [Pattern::matches_repo_relative_path()].
#[derive(Debug, PartialOrd, PartialEq, Copy, Clone, Hash, Ord, Eq)]
pub enum Case {
    /// The case affects the match
    Sensitive,
    /// Ignore the case of ascii characters.
    Fold,
}

impl Pattern {
    /// Parse the given `text` as pattern, or return `None` if `text` was empty.
    pub fn from_bytes(text: &[u8]) -> Option<Self> {
        crate::parse::pattern(text).map(|(text, mode, first_wildcard_pos)| Pattern {
            text,
            mode,
            first_wildcard_pos,
        })
    }

    /// Return true if a match is negated.
    pub fn is_negative(&self) -> bool {
        self.mode.contains(Mode::NEGATIVE)
    }

    /// Match the given `path` which takes slashes (and only slashes) literally, and is relative to the repository root.
    /// Note that `path` is assumed to be relative to the repository, and that `base_path` is assumed to contain `path`
    /// and is also relative to the repository.
    ///
    /// We may take various shortcuts which is when `basename_start_pos` and `is_dir` come into play.
    /// `basename_start_pos` is the index at which the `path`'s basename starts.
    ///
    /// Lastly, `case` folding can be configured as well.
    ///
    /// Note that this method uses shortcuts to accelerate simple patterns.
    pub fn matches_repo_relative_path<'a>(
        &self,
        path: impl Into<&'a BStr>,
        basename_start_pos: Option<usize>,
        base_path: Option<&BStr>,
        is_dir: bool,
        case: Case,
    ) -> bool {
        if !is_dir && self.mode.contains(pattern::Mode::MUST_BE_DIR) {
            return false;
        }

        let flags = wildmatch::Mode::NO_MATCH_SLASH_LITERAL
            | match case {
                Case::Fold => wildmatch::Mode::IGNORE_CASE,
                Case::Sensitive => wildmatch::Mode::empty(),
            };
        let path = path.into();
        debug_assert_eq!(
            basename_start_pos,
            path.rfind_byte(b'/').map(|p| p + 1),
            "BUG: invalid cached basename_start_pos provided"
        );
        debug_assert!(
            base_path.map_or(true, |p| p.ends_with(b"/")),
            "base must end with a trailing slash"
        );
        debug_assert!(!path.starts_with(b"/"), "input path must be relative");
        debug_assert!(
            base_path.map(|base| path.starts_with(base)).unwrap_or(true),
            "repo-relative paths must be pre-filtered to match our base."
        );

        let (text, first_wildcard_pos) = self
            .mode
            .contains(pattern::Mode::ABSOLUTE)
            .then(|| (self.text[1..].as_bstr(), self.first_wildcard_pos.map(|p| p - 1)))
            .unwrap_or((self.text.as_bstr(), self.first_wildcard_pos));
        if self.mode.contains(pattern::Mode::NO_SUB_DIR) {
            let basename = if self.mode.contains(pattern::Mode::ABSOLUTE) {
                base_path
                    .and_then(|base| path.strip_prefix(base.as_ref()).map(|b| b.as_bstr()))
                    .unwrap_or(path)
            } else {
                &path[basename_start_pos.unwrap_or_default()..]
            };
            self.matches_inner(text, first_wildcard_pos, basename, flags)
        } else {
            let path = match base_path {
                Some(base) => match path.strip_prefix(base.as_ref()) {
                    Some(path) => path.as_bstr(),
                    None => return false,
                },
                None => path,
            };
            self.matches_inner(text, first_wildcard_pos, path, flags)
        }
    }

    /// See if `value` matches this pattern in the given `mode`.
    ///
    /// `mode` can identify `value` as path which won't match the slash character, and can match
    /// strings with cases ignored as well. Note that the case folding performed here is ASCII only.
    ///
    /// Note that this method uses some shortcuts to accelerate simple patterns.
    pub fn matches<'a>(&self, value: impl Into<&'a BStr>, mode: wildmatch::Mode) -> bool {
        self.matches_inner(self.text.as_bstr(), self.first_wildcard_pos, value, mode)
    }

    fn matches_inner<'a>(
        &self,
        text: &BStr,
        first_wildcard_pos: Option<usize>,
        value: impl Into<&'a BStr>,
        mode: wildmatch::Mode,
    ) -> bool {
        let value = value.into();
        match first_wildcard_pos {
            // "*literal" case, overrides starts-with
            Some(pos) if self.mode.contains(pattern::Mode::ENDS_WITH) && !value.contains(&b'/') => {
                let text = &text[pos + 1..];
                if mode.contains(wildmatch::Mode::IGNORE_CASE) {
                    value
                        .len()
                        .checked_sub(text.len())
                        .map(|start| text.eq_ignore_ascii_case(&value[start..]))
                        .unwrap_or(false)
                } else {
                    value.ends_with(text.as_ref())
                }
            }
            Some(pos) => {
                if mode.contains(wildmatch::Mode::IGNORE_CASE) {
                    if !value
                        .get(..pos)
                        .map_or(false, |value| value.eq_ignore_ascii_case(&text[..pos]))
                    {
                        return false;
                    }
                } else if !value.starts_with(&text[..pos]) {
                    return false;
                }
                crate::wildmatch(text.as_bstr(), value, mode)
            }
            None => {
                if mode.contains(wildmatch::Mode::IGNORE_CASE) {
                    text.eq_ignore_ascii_case(value)
                } else {
                    text == value
                }
            }
        }
    }
}
