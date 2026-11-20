use std::borrow::Cow;

type Labels = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
);
const TAGS: &[Labels] = &[
    (
        "악기",
        "Instrument",
        &[
            ("기타", "Guitar"),
            ("드럼", "Drums"),
            ("라이드", "Ride"),
            ("림샷", "Rimshot"),
            ("베이스", "Bass"),
            ("보컬", "Vocal"),
            ("셰이커", "Shaker"),
            ("스냅", "Snap"),
            ("스네어", "Snare"),
            ("신스", "Synth"),
            ("심벌", "Cymbal"),
            ("오픈 햇", "Open hi-hat"),
            ("카우벨", "Cowbell"),
            ("콩가·봉고", "Conga / bongo"),
            ("크래시", "Crash"),
            ("클라베", "Clave"),
            ("클랩", "Clap"),
            ("클로즈드 햇", "Closed hi-hat"),
            ("킥", "Kick"),
            ("탐", "Tom"),
            ("탬버린", "Tambourine"),
            ("퍼커션", "Percussion"),
            ("피아노·건반", "Piano / keys"),
            ("하이햇", "Hi-hat"),
            ("현악기", "Strings"),
            ("효과음", "Sound effect"),
            ("미분류", "Unclassified"),
        ],
    ),
    (
        "장르",
        "Genre",
        &[
            ("힙합", "Hip-hop"),
            ("트랩", "Trap"),
            ("드릴", "Drill"),
            ("하우스", "House"),
            ("테크노", "Techno"),
            ("드럼 앤 베이스", "Drum and bass"),
            ("R&B", "R&B"),
            ("재즈", "Jazz"),
            ("로파이", "Lo-fi"),
            ("팝", "Pop"),
            ("록", "Rock"),
            ("앰비언트", "Ambient"),
            ("UK 개러지", "UK garage"),
            ("아프로비츠", "Afrobeats"),
            ("레게", "Reggae"),
            ("레게톤", "Reggaeton"),
            ("댄스홀", "Dancehall"),
            ("소카", "Soca"),
            ("퐁크", "Phonk"),
            ("디스코", "Disco"),
            ("펑크", "Funk"),
            ("고스펠", "Gospel"),
            ("미분류", "Unclassified"),
        ],
    ),
    (
        "길이",
        "Length",
        &[
            ("1초 미만", "Under 1 s"),
            ("1–8초", "1–8 s"),
            ("8초 이상", "8 s and up"),
        ],
    ),
    (
        "밝기",
        "Brightness",
        &[
            ("어두움", "Dark"),
            ("중간", "Medium"),
            ("밝음", "Bright"),
            ("미분류", "Unclassified"),
        ],
    ),
    (
        "형태",
        "Type",
        &[
            ("루프 후보", "Likely loop"),
            ("원샷 후보", "Likely one-shot"),
            ("미분류", "Unclassified"),
        ],
    ),
];

pub(super) fn tag_parts(value: &str) -> Option<(&'static str, &'static str)> {
    let (category, label) = value.split_once(" · ")?;
    let (_, display, labels) = TAGS.iter().find(|(stored, _, _)| *stored == category)?;
    let (_, text) = labels.iter().find(|(key, _)| *key == label)?;
    Some((display, text))
}

pub(super) fn instrument_subgroup(value: &str) -> Option<&'static str> {
    let instrument = value.strip_prefix("악기 · ")?;
    Some(match instrument {
        "드럼" | "킥" | "스네어" | "클랩" | "림샷" | "탐" | "클로즈드 햇" | "오픈 햇"
        | "하이햇" | "라이드" | "크래시" | "심벌" => "Drums",
        "퍼커션" | "셰이커" | "탬버린" | "카우벨" | "콩가·봉고" | "클라베" | "스냅" => {
            "Percussion"
        }
        _ => "Other instruments",
    })
}

pub(super) fn tag(value: &str) -> Cow<'_, str> {
    match tag_parts(value) {
        Some((category, label)) => Cow::Owned(format!("{category} · {label}")),
        None => Cow::Borrowed(value),
    }
}

pub(super) fn stored_tag(value: String) -> String {
    if let Some((category, label)) = value.split_once(" · ") {
        for (stored, display, labels) in TAGS {
            if category.eq_ignore_ascii_case(display)
                && let Some((key, _)) = labels
                    .iter()
                    .find(|(_, text)| label.eq_ignore_ascii_case(text))
            {
                return format!("{stored} · {key}");
            }
        }
    }
    value
}

pub(super) fn version(value: &str) -> &str {
    if value == "버전 미지정" {
        "Unversioned"
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translated_tags_round_trip_without_changing_custom_tags() {
        for (category, _, values) in TAGS {
            for (label, _) in *values {
                let original = format!("{category} · {label}");
                assert_eq!(stored_tag(tag(&original).into_owned()), original);
            }
        }
        for user_tag in ["내 드럼", "Genre · Playlist 1", "장르 · Mix 04"] {
            assert_eq!(tag(user_tag), user_tag);
            assert_eq!(stored_tag(user_tag.to_owned()), user_tag);
        }
    }

    #[test]
    fn instrument_subgroups_separate_drum_kit_and_percussion() {
        for tag in ["악기 · 킥", "악기 · 스네어", "악기 · 클로즈드 햇"] {
            assert_eq!(instrument_subgroup(tag), Some("Drums"));
        }
        for tag in ["악기 · 셰이커", "악기 · 콩가·봉고"] {
            assert_eq!(instrument_subgroup(tag), Some("Percussion"));
        }
        assert_eq!(
            instrument_subgroup("악기 · 기타"),
            Some("Other instruments")
        );
        assert_eq!(instrument_subgroup("장르 · 하우스"), None);
    }
}
