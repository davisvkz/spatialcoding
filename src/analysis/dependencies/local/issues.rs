//! Why an import produced no link, for every language.
//!
//! A missing edge has to be explainable: the file may import a package (not
//! drawn on purpose), a path that does not exist, or something the analysis
//! does not follow. Packages are only counted (a big repository imports
//! thousands); everything else is listed, up to [`MAX_ISSUES`], with exact
//! counts by reason.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::analysis::cst::CstLanguage;

/// The most issues kept; the counts stay exact beyond it.
pub const MAX_ISSUES: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ImportIssueReason {
    /// A package, a built-in or the standard library: not a file of the
    /// project, so no link is drawn.
    External,
    /// No file of the folder is meant by it.
    NotFound,
    /// More than one file is.
    Ambiguous,
    /// A configuration needed to tell is missing or could not be read.
    MissingContext,
    /// The path leaves the workspace.
    OutsideWorkspace,
    /// Behind `cfg` (Rust).
    Conditional,
    /// A form the analysis does not follow (a URL, an attribute, a package
    /// of several files…).
    Unsupported,
}

impl ImportIssueReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::External => "External package",
            Self::NotFound => "No file for this import in the folder",
            Self::Ambiguous => "Multiple targets in the folder",
            Self::MissingContext => "Needs a configuration that is missing or unreadable",
            Self::OutsideWorkspace => "Path leaves the workspace",
            Self::Conditional => "Conditional module (cfg or cfg_attr)",
            Self::Unsupported => "Form that is not followed",
        }
    }

    /// Short name for the counts.
    pub fn short(self) -> &'static str {
        match self {
            Self::External => "external",
            Self::NotFound => "not found",
            Self::Ambiguous => "ambiguous",
            Self::MissingContext => "missing context",
            Self::OutsideWorkspace => "outside",
            Self::Conditional => "conditional",
            Self::Unsupported => "not followed",
        }
    }
}

/// One import that gave no link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportIssue {
    /// Relative to the project folder.
    pub path: PathBuf,
    /// 1-based.
    pub line: usize,
    /// What was imported, as written (a module name for Rust).
    pub specifier: String,
    /// `None` for Markdown and JSON.
    pub language: Option<CstLanguage>,
    pub reason: ImportIssueReason,
}

/// The issues of one analysis.
#[derive(Clone, Debug, Default)]
pub struct ImportIssues {
    items: Vec<ImportIssue>,
    counts: BTreeMap<ImportIssueReason, usize>,
}

impl ImportIssues {
    /// Records an issue; a package is only counted.
    pub fn report(&mut self, issue: ImportIssue) {
        *self.counts.entry(issue.reason).or_default() += 1;
        if issue.reason != ImportIssueReason::External {
            self.items.push(issue);
        }
    }

    /// Counts a package import.
    pub fn external(&mut self) {
        *self.counts.entry(ImportIssueReason::External).or_default() += 1;
    }

    /// Adds what another file found.
    pub fn extend(&mut self, other: ImportIssues) {
        self.items.extend(other.items);
        for (reason, count) in other.counts {
            *self.counts.entry(reason).or_default() += count;
        }
    }

    /// Orders the issues by place and drops those past [`MAX_ISSUES`].
    pub fn finish(&mut self) {
        self.items.sort_by(|a, b| (&a.path, a.line, &a.specifier).cmp(&(&b.path, b.line, &b.specifier)));
        self.items.truncate(MAX_ISSUES);
    }

    /// The listed issues (never packages), in file and line order.
    pub fn items(&self) -> &[ImportIssue] {
        &self.items
    }

    pub fn count(&self, reason: ImportIssueReason) -> usize {
        self.counts.get(&reason).copied().unwrap_or(0)
    }

    /// Imports that gave no link and are not packages.
    pub fn unresolved(&self) -> usize {
        self.counts.iter().filter(|(reason, _)| **reason != ImportIssueReason::External).map(|(_, n)| n).sum()
    }

    /// The counts of the reasons that occurred, packages included.
    pub fn counts(&self) -> impl Iterator<Item = (ImportIssueReason, usize)> + '_ {
        self.counts.iter().map(|(reason, n)| (*reason, *n))
    }

    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }
}
