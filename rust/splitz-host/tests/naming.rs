//! What a participant is called on screen (§9.1).

use std::collections::BTreeSet;

use splitz_core::{Bill, Participant};
use splitz_host::{display_name_of, name_skeleton, shared_names};

/// One table, pinned in both host packages.
const SKELETONS: &str = r#"[["Ana", "ana"], ["ANA", "ana"], ["\u0410na", "ana"], ["\u0430n\u0430", "ana"], ["Ana\u200b", "ana"], ["A\u0301na", "ana"], ["  Ana   Ben ", "ana ben"], ["\uff22en", "ben"], ["\ud835\udc01en", "ben"], ["\ud835\udfcf\ud835\udfd0", "i2"], ["\u0392\u03b5\u03bd", "bev"], ["\u03a3\u0399\u03a3\u03a5\u03a6\u039f\u03a3", "\u03c3i\u03c3y\u03c6o\u03c3"], ["\u0130stanbul", "istanbui"], ["\u01c0ucy", "iucy"], ["\u0391\u039d\u0391", "ana"], ["\u041e\u043b\u0435\u0433", "o\u043be\u0433"], ["stra\u00dfe", "stra\u00dfe"], ["\u01c5", "\u01c6"], ["Ana\u3000Ben", "ana ben"], ["\ufeffAna", "ana"], ["Ana\udb40\udc41", "ana"], ["\u0397ANS", "hans"], ["HANS", "hans"], ["AIex", "aiex"], ["Alex", "aiex"], ["Ana\u2002Lee", "ana iee"], ["\u039d\u03a5", "ny"]]"#;

fn bill(people: &[(&str, &str)]) -> Bill {
    Bill {
        id: "b".into(),
        name: "Dinner".into(),
        currency: "EUR".into(),
        split_mode: "equal".into(),
        participants: people
            .iter()
            .map(|(id, name)| Participant {
                id: (*id).into(),
                name: (*name).into(),
                pay_to: None,
                identity_key: None,
                payouts: vec![],
            })
            .collect(),
        expenses: vec![],
        payments: vec![],
        confirmed_payments: BTreeSet::new(),
        rate: None,
    }
}

#[test]
fn a_name_folds_to_what_a_reader_sees() {
    let pairs: Vec<(String, String)> = serde_json::from_str(SKELETONS).unwrap();
    for (name, skeleton) in pairs {
        assert_eq!(name_skeleton(&name), skeleton, "{name}");
    }
}

#[test]
fn only_a_colliding_name_is_qualified_the_organiser_by_role() {
    let b = bill(&[
        ("ana-1111111111aaaaaaaa", "Ana"),
        ("ana-2222222222bbbbbbbb", "\u{410}na"),
        ("ben", "Ben"),
    ]);
    assert_eq!(display_name_of(&b, "ben", None), "Ben");
    assert_eq!(
        display_name_of(&b, "ana-1111111111aaaaaaaa", Some("ana-1111111111aaaaaaaa")),
        "Ana (organiser)"
    );
    assert_eq!(
        display_name_of(&b, "ana-2222222222bbbbbbbb", None),
        "\u{410}na (…bbbbbbbb)"
    );
    assert_eq!(
        shared_names(&b),
        vec!["Ana".to_owned(), "\u{410}na".to_owned()]
    );
    assert_eq!(display_name_of(&b, "nobody-12345678", None), "…5678");
}

#[test]
fn an_id_copying_anothers_last_eight_is_shown_whole() {
    let b = bill(&[("xx-aaaaaaaa", "Ana"), ("yy-aaaaaaaa", "Ana")]);
    assert_eq!(
        display_name_of(&b, "yy-aaaaaaaa", None),
        "Ana (yy-aaaaaaaa)"
    );
}

#[test]
fn a_letter_written_precomposed_or_decomposed_is_one_letter() {
    // U+00C1 and A + U+0301 render alike; both fold to a.
    assert_eq!(name_skeleton("\u{C1}na"), "ana");
    assert_eq!(name_skeleton("A\u{301}na"), "ana");
    assert_eq!(name_skeleton("Ren\u{E9}e"), name_skeleton("Renee"));
    assert_eq!(name_skeleton("\u{1FA}s"), "as");
    // Letters with no decomposition stay themselves.
    assert_eq!(name_skeleton("Stra\u{DF}e"), "stra\u{DF}e");
    assert_eq!(name_skeleton("\u{E6}\u{111}\u{F8}"), "\u{E6}\u{111}\u{F8}");
    assert_eq!(name_skeleton("\u{234}"), "\u{234}");
    let b = bill(&[("xx-aaaaaaaa", "\u{C1}na"), ("yy-bbbbbbbb", "A\u{301}na")]);
    assert_eq!(shared_names(&b).len(), 2);
}
