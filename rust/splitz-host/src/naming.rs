//! What a participant is called on screen, and which names a reader cannot
//! tell apart (§9.1).
//!
//! A display name is chosen by whoever joins and can be chosen twice, or
//! chosen to pass as somebody already on the bill. A collision cannot move
//! money; it can make the record of who did what unreadable. Every wallet
//! qualifies the same names the same way, so one bill reads alike on each.

use std::collections::{BTreeMap, BTreeSet};

use splitz_core::{compare_utf8, Bill};

/// The tail of an id, for a reader who needs to tell two apart: its last
/// `length` characters, counted as Unicode scalar values (§2.3), or the whole
/// id when that is hardly longer.
pub fn short_id(id: &str, length: usize) -> String {
    let chars: Vec<char> = id.chars().collect();
    if chars.len() <= length + 2 {
        return id.to_owned();
    }
    format!(
        "…{}",
        chars[chars.len() - length..].iter().collect::<String>()
    )
}

/// A participant's name, made unambiguous when more than one answers to it.
///
/// Only a colliding name is qualified, so the ordinary case stays plain. The
/// one who opened the bill is named as the organiser rather than by a
/// fragment of an id, because that is the half a reader can act on. Eight
/// characters otherwise: a key's id is a digest (§10.7), and matching eight of
/// its characters takes 2^48 tries where four take 2^24. An id nobody's key
/// derives is chosen freely, so one that copies another's eight is shown
/// whole: ids are unique on a bill.
pub fn display_name_of(bill: &Bill, id: &str, creator_id: Option<&str>) -> String {
    let Some(person) = bill.participant(id) else {
        return short_id(id, 4);
    };
    let name = if person.name.is_empty() {
        short_id(id, 4)
    } else {
        person.name.clone()
    };
    let skeleton = name_skeleton(&person.name);
    let alike: Vec<_> = bill
        .participants
        .iter()
        .filter(|p| name_skeleton(&p.name) == skeleton)
        .collect();
    if alike.len() < 2 {
        return name;
    }
    if creator_id == Some(id) {
        return format!("{name} (organiser)");
    }
    let tail = short_id(id, 8);
    let copied = alike
        .iter()
        .any(|p| p.id != id && short_id(&p.id, 8) == tail);
    format!("{name} ({})", if copied { id.to_owned() } else { tail })
}

/// Every display name more than one participant answers to, in §2.3's order.
/// Two names count as one when a reader cannot tell them apart: see
/// [`name_skeleton`].
pub fn shared_names(bill: &Bill) -> Vec<String> {
    let mut first: BTreeMap<String, String> = BTreeMap::new();
    let mut shared: BTreeSet<String> = BTreeSet::new();
    for p in &bill.participants {
        if p.name.is_empty() {
            continue;
        }
        let skeleton = name_skeleton(&p.name);
        match first.get(&skeleton) {
            None => {
                first.insert(skeleton, p.name.clone());
            }
            Some(held) => {
                shared.insert(held.clone());
                shared.insert(p.name.clone());
            }
        }
    }
    let mut out: Vec<String> = shared.into_iter().collect();
    out.sort_by(|a, b| compare_utf8(a, b));
    out
}

/// `name` as a reader sees it, for telling whether two names can be told
/// apart.
///
/// Case folded within [`CASE_FOLDED`] — Latin, Greek, Cyrillic, Armenian and Latin
/// Extended Additional, less the few pairs encoded after the rest — and
/// nowhere else, so every implementation folds a name alike whatever Unicode
/// version its library carries; invisible format characters and combining
/// marks removed; runs
/// of space collapsed; and the Cyrillic and Greek letters that render as Latin
/// ones mapped to them. A look-alike chosen to pass as somebody already on the
/// bill then collides with them, and both are qualified. Not every confusable
/// Unicode knows: the common ones a name would be forged with.
pub fn name_skeleton(name: &str) -> String {
    let mut out = String::new();
    let mut space = false;
    for c in name.chars().flat_map(|c| {
        let folded = case_folded(c as u32);
        c.to_lowercase()
            .filter(move |_| folded)
            .chain((!folded).then_some(c))
    }) {
        let r = c as u32;
        if invisible(r) || combining(r) {
            continue;
        }
        if matches!(r, 0x20 | 0x09 | 0xA0 | 0x3000) {
            space = !out.is_empty();
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        out.push(char::from_u32(look_alike(r)).unwrap_or(c));
    }
    out
}

/// The code points [`name_skeleton`] lower-cases, as inclusive ranges.
const CASE_FOLDED: [(u32, u32); 5] = [
    (0x0000, 0x024F),
    (0x0370, 0x037E),
    (0x0380, 0x0523),
    (0x0531, 0x0556),
    (0x1E00, 0x1FFF),
];

fn case_folded(r: u32) -> bool {
    CASE_FOLDED.iter().any(|&(lo, hi)| (lo..=hi).contains(&r))
}

fn invisible(r: u32) -> bool {
    matches!(r,
        0xAD | 0x34F | 0x61C | 0x115F..=0x1160 | 0x17B4..=0x17B5 | 0x180B..=0x180F
        | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x206F | 0x3164
        | 0xFE00..=0xFE0F | 0xFEFF | 0xFFA0 | 0xE0000..=0xE0FFF)
}

fn combining(r: u32) -> bool {
    matches!(r,
        0x300..=0x36F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

/// Lower-case Cyrillic and Greek letters that render as a Latin one.
const LATIN_LOOK_ALIKE: [(u32, u32); 31] = [
    (0x430, 0x61),
    (0x432, 0x62),
    (0x435, 0x65),
    (0x456, 0x69),
    (0x458, 0x6A),
    (0x43A, 0x6B),
    (0x43C, 0x6D),
    (0x43D, 0x68),
    (0x43E, 0x6F),
    (0x440, 0x70),
    (0x441, 0x63),
    (0x442, 0x74),
    (0x443, 0x79),
    (0x445, 0x78),
    (0x455, 0x73),
    (0x4CF, 0x6C),
    (0x3B1, 0x61),
    (0x3B5, 0x65),
    (0x3B9, 0x69),
    (0x3BA, 0x6B),
    (0x3BD, 0x76),
    (0x3BF, 0x6F),
    (0x3C1, 0x70),
    (0x3C4, 0x74),
    (0x3C5, 0x75),
    (0x3C7, 0x78),
    (0x131, 0x69),
    (0x1C0, 0x6C),
    (0x251, 0x61),
    (0x261, 0x67),
    (0x269, 0x69),
];

/// The Latin letters U+00C0..=U+0233 that decompose to a plain letter and
/// combining marks, folded to that letter: index `r - 0xC0`, `.` where none.
/// The decomposed spelling of the same letter loses its marks in
/// [`combining`], so both spellings of one name meet.
const PRECOMPOSED_BASE: &[u8; 372] = b"\
aaaaaa.ceeeeiiii.nooooo..uuuuy..aaaaaa.ceeeeiiii.nooooo..uuuuy\
.yaaaaaaccccccccdd..eeeeeeeeeegggggggghh..iiiiiiiii...jjkk.lll\
lll....nnnnnn...oooooo..rrrrrrsssssssstttt..uuuuuuuuuuuuwwyyyz\
zzzzz.................................oo.............uu.......\
.....................aaiioouuuuuuuuuu.aaaa....ggkkoooo..j...gg\
..nnaa....aaaaeeeeiiiioooorrrruuuusstt..hh......aaeeooooooooyy";

/// `rune` as the plain letter or digit it renders as, when it is one.
///
/// Fullwidth forms are ASCII at another width; the mathematical alphanumerics
/// are letters in runs of 52, capitals then small, and digits in runs of 10.
fn look_alike(rune: u32) -> u32 {
    let mut r = rune;
    if (0xFF01..=0xFF5E).contains(&r) {
        r -= 0xFEE0;
    } else if (0x1D400..=0x1D6A3).contains(&r) {
        let i = (r - 0x1D400) % 52;
        r = 0x61 + if i < 26 { i } else { i - 26 };
    } else if (0x1D7CE..=0x1D7FF).contains(&r) {
        r = 0x30 + (r - 0x1D7CE) % 10;
    }
    if (0x41..=0x5A).contains(&r) {
        r += 0x20;
    }
    if let Some(&base) = r
        .checked_sub(0xC0)
        .and_then(|i| PRECOMPOSED_BASE.get(i as usize))
    {
        if base != b'.' {
            r = u32::from(base);
        }
    }
    LATIN_LOOK_ALIKE
        .iter()
        .find(|(from, _)| *from == r)
        .map_or(r, |(_, to)| *to)
}
