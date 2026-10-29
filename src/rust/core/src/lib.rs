// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

pub mod artifact;
pub mod collection;
pub mod extract;
pub mod index;
mod json;
pub mod markup;
mod names;
pub mod pep440;
pub mod plugins;
pub mod semver;
mod text;
pub mod toml;
pub mod yaml;

use std::fmt;

/// Text that isn't a valid value of what it was parsed as, with the message to show for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError(String);

impl ParseError {
    fn new(message: impl Into<String>) -> Self {
        ParseError(message.into())
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}
