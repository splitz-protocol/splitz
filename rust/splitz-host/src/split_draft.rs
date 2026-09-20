//! A split being edited, and the §4 payload it becomes.
//!
//! The protocol defines five ways to divide an expense and refuses a sixth. A
//! form cannot hold a §4 payload directly — half-typed input is not a split —
//! so this holds what a person has entered so far and answers two questions:
//! what would this become, and what is wrong with it now.
//!
//! **The protocol decides what is wrong, not this file.** Every refusal here
//! comes from building the real payload and splitting a real expense with it,
//! so a form can never accept something the fold would set aside, and can
//! never invent a rule §4 does not have.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};
use splitz_core::{split_expense, Shares};

/// The five, and only the five (§4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitKind {
    Equal,
    Exact,
    Percentage,
    Shares,
    Itemized,
}

impl SplitKind {
    /// Every kind, in the order a form offers them.
    pub const ALL: [SplitKind; 5] = [
        SplitKind::Equal,
        SplitKind::Exact,
        SplitKind::Percentage,
        SplitKind::Shares,
        SplitKind::Itemized,
    ];

    /// The §4 discriminator.
    ///
    /// What a person is shown for each kind is the wallet's: a name written
    /// here would be English only, and every wallet that is not in English
    /// would carry a second one anyway.
    pub fn wire_type(&self) -> &'static str {
        match self {
            SplitKind::Equal => "equal",
            SplitKind::Exact => "exact",
            SplitKind::Percentage => "percentage",
            SplitKind::Shares => "shares",
            SplitKind::Itemized => "itemized",
        }
    }
}

/// One line of an itemized split (§4.5).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DraftItem {
    pub description: String,
    pub minor_units: i64,
    /// Who ate it. §4.5 refuses an item assigned to nobody.
    pub shared_by: BTreeSet<String>,
}

/// What a person has entered, and what it would become.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitDraft {
    pub kind: SplitKind,
    /// Who shares it, for [`SplitKind::Equal`].
    pub among: BTreeSet<String>,
    /// Minor units per participant, for [`SplitKind::Exact`].
    pub amounts: BTreeMap<String, i64>,
    /// Hundredths of a percent per participant, for [`SplitKind::Percentage`].
    ///
    /// Basis points rather than percentages so a share is an integer at every
    /// step: 33.33% is 3333, and no float ever touches an amount.
    pub basis_points: BTreeMap<String, i64>,
    /// Weights per participant, for [`SplitKind::Shares`].
    pub share_counts: BTreeMap<String, i64>,
    /// The lines, for [`SplitKind::Itemized`].
    pub items: Vec<DraftItem>,
    /// Tax, tip or service, for [`SplitKind::Itemized`]. Allocated across
    /// people in proportion to what they ate (§4.5).
    pub extra_minor_units: i64,
}

impl SplitDraft {
    pub fn new(kind: SplitKind) -> Self {
        Self {
            kind,
            among: BTreeSet::new(),
            amounts: BTreeMap::new(),
            basis_points: BTreeMap::new(),
            share_counts: BTreeMap::new(),
            items: Vec::new(),
            extra_minor_units: 0,
        }
    }

    /// The §4 payload this would become.
    ///
    /// Built whatever state the form is in: it is the protocol's job to say a
    /// payload is wrong, and building only "valid" ones here would mean this
    /// file deciding what valid means.
    pub fn to_split(&self) -> Value {
        fn weights(of: &BTreeMap<String, i64>) -> Value {
            let mut out = Map::new();
            for (id, value) in of {
                out.insert(id.clone(), Value::from(*value));
            }
            Value::Object(out)
        }
        match self.kind {
            SplitKind::Equal => json!({
                "type": "equal",
                "among": self.among.iter().cloned().collect::<Vec<_>>(),
            }),
            SplitKind::Exact => json!({
                "type": "exact",
                "amounts": weights(&self.amounts),
            }),
            SplitKind::Percentage => json!({
                "type": "percentage",
                "basisPoints": weights(&self.basis_points),
            }),
            SplitKind::Shares => json!({
                "type": "shares",
                "shareCounts": weights(&self.share_counts),
            }),
            SplitKind::Itemized => json!({
                "type": "itemized",
                "extraMinorUnits": self.extra_minor_units,
                "items": self.items.iter().map(|item| json!({
                    "description": item.description,
                    "minorUnits": item.minor_units,
                    "sharedBy": item.shared_by.iter().cloned().collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            }),
        }
    }

    /// What each participant would owe of `total_minor_units`, or `None` when
    /// the protocol refuses this split.
    ///
    /// The allocation is the protocol's — largest remainder, exact integers —
    /// and is never reimplemented here. A form that did its own arithmetic
    /// would show a person a figure the bill then disagreed with.
    pub fn allocation(&self, total_minor_units: i64) -> Option<Shares> {
        split_expense(total_minor_units, &self.to_split()).ok()
    }

    /// Why the protocol will not accept this yet, as its §12 code, or `None`
    /// when it will.
    ///
    /// The code, not a sentence. §1 says the code is what a wallet turns into
    /// a sentence for its user, and one written here would be English only.
    pub fn refusal_code(&self, total_minor_units: i64) -> Option<&'static str> {
        split_expense(total_minor_units, &self.to_split())
            .err()
            .map(|e| e.code)
    }

    /// Everyone this draft names, whichever kind it is.
    pub fn participants(&self) -> BTreeSet<String> {
        match self.kind {
            SplitKind::Equal => self.among.clone(),
            SplitKind::Exact => self.amounts.keys().cloned().collect(),
            SplitKind::Percentage => self.basis_points.keys().cloned().collect(),
            SplitKind::Shares => self.share_counts.keys().cloned().collect(),
            SplitKind::Itemized => self
                .items
                .iter()
                .flat_map(|item| item.shared_by.iter().cloned())
                .collect(),
        }
    }

    /// Puts `id` in or out of the split, whichever kind it is.
    ///
    /// Removing somebody drops the figure they carried rather than leaving it
    /// behind: a stale amount for a person no longer in the split is exactly
    /// what `exact_total_mismatch` is.
    pub fn toggle(&mut self, id: &str) {
        match self.kind {
            SplitKind::Equal => {
                if !self.among.remove(id) {
                    self.among.insert(id.to_owned());
                }
            }
            SplitKind::Exact => toggle_weight(&mut self.amounts, id, 0),
            SplitKind::Percentage => toggle_weight(&mut self.basis_points, id, 0),
            // Zero shares owes nothing, and every count zero is refused
            // outright, so somebody added to a shares split starts on one.
            SplitKind::Shares => toggle_weight(&mut self.share_counts, id, 1),
            SplitKind::Itemized => {
                for item in &mut self.items {
                    if !item.shared_by.remove(id) {
                        item.shared_by.insert(id.to_owned());
                    }
                }
            }
        }
    }
}

fn toggle_weight(weights: &mut BTreeMap<String, i64>, id: &str, when_added: i64) {
    if weights.remove(id).is_none() {
        weights.insert(id.to_owned(), when_added);
    }
}
