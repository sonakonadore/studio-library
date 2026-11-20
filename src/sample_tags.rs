use crate::scope::{key, name};
use std::path::Path;

const UNKNOWN: &str = "미분류";

const DRUMS: &[(&str, &str)] = &[
    ("킥", "kick|kicks|kck|bd|bassdrum|bass drum|킥"),
    ("스네어", "snare|snares|snr|sd|스네어"),
    ("클랩", "clap|claps|handclap|hand clap|hand claps|clp|클랩"),
    (
        "림샷",
        "rimshot|rimshots|rim shot|rim shots|rim|rims|rim click|sidestick|side stick|림샷",
    ),
    (
        "클로즈드 햇",
        "closed hat|closed hats|hat closed|hats closed|closed hi hat|hi hat closed|closed hihat|hihat closed|closedhat|closedhihat|hatclosed|hihatclosed|closed hh|hh closed|chh|clhat|클로즈드 햇",
    ),
    (
        "오픈 햇",
        "open hat|open hats|hat open|hats open|open hi hat|hi hat open|open hihat|hihat open|openhat|openhihat|hatopen|hihatopen|open hh|hh open|ohh|ophat|오픈 햇",
    ),
    ("셰이커", "shaker|shakers|shk|maraca|maracas|셰이커"),
    ("탬버린", "tamb|tambourine|tambourines|탬버린"),
    ("탐", "tom|toms|tomtom|tom tom|탐"),
    ("카우벨", "cowbell|cow bell|카우벨"),
    ("콩가·봉고", "conga|congas|bongo|bongos"),
    ("클라베", "clave|claves"),
    ("스냅", "finger snap|finger snaps|fingersnap|snap|snaps"),
    ("라이드", "ride cymbal|ride cymbals|ride|rides|라이드"),
    ("크래시", "crash cymbal|crash cymbals|crash|crashes|크래시"),
];

const INSTRUMENTS: &[(&str, &str)] = &[
    ("하이햇", "hi hat|hi hats|hihat|hihats|hh|hat|hats|하이햇"),
    ("심벌", "cymbal|cymbals|심벌"),
    ("퍼커션", "perc|percussion|퍼커션"),
    ("드럼", "drum|drums|drumkit|drum kit|드럼"),
    (
        "베이스",
        "bass|basses|sub bass|subbass|808 sub|sub 808|베이스",
    ),
    ("신스", "synth|synths|lead|pad|arpeggio|신스"),
    ("피아노·건반", "keys|keyboard|piano|rhodes|organ|피아노"),
    ("기타", "guitar|guitars|gtr|기타"),
    ("보컬", "vocal|vocals|vox|voice|acapella|보컬"),
    ("현악기", "strings|string|violin|cello"),
    ("효과음", "fx|sfx|foley|효과음"),
];

const GENRES: &[(&str, &str)] = &[
    ("힙합", "hiphop|hip hop|boombap|boom bap|힙합"),
    ("트랩", "trap|trap music|트랩"),
    ("드릴", "drill beats|uk drill|brooklyn drill|드릴"),
    (
        "하우스",
        "deep house|tech house|acid house|disco house|afro house|garage house|house music|하우스",
    ),
    ("테크노", "techno|테크노"),
    (
        "드럼 앤 베이스",
        "dnb|drum and bass|drumnbass|jungle break|jungle breaks|jungle dnb",
    ),
    ("R&B", "rnb|r b|rhythm and blues"),
    ("재즈", "jazz|재즈"),
    ("로파이", "lofi|lo fi|로파이"),
    ("팝", "pop music|kpop|k pop|synthpop|synth pop|팝"),
    ("록", "rock music|indie rock|hard rock|punk rock|록"),
    ("앰비언트", "ambient music|ambient electronic|앰비언트"),
    ("UK 개러지", "ukg|uk garage|ukg garage"),
    ("아프로비츠", "afrobeats|afrobeat|afro beats|afro beat"),
    ("레게", "reggae|레게"),
    ("레게톤", "reggaeton|레게톤"),
    ("댄스홀", "dancehall|dance hall|댄스홀"),
    ("소카", "soca|소카"),
    ("퐁크", "phonk|퐁크"),
    ("디스코", "disco music|디스코"),
    ("펑크", "funk music|펑크"),
    ("고스펠", "gospel|고스펠"),
];

const EXPLICIT_GENRES: &[(&str, &str)] = &[
    ("하우스", "house"),
    ("팝", "pop"),
    ("록", "rock|punk|metal"),
    ("드럼 앤 베이스", "jungle"),
    ("드릴", "drill"),
    ("앰비언트", "ambient"),
    ("디스코", "disco"),
    ("펑크", "funk"),
    ("힙합", "rap"),
];

#[derive(Default)]
pub(crate) struct MetadataWords {
    pub text: Vec<String>,
    pub instruments: Vec<String>,
    pub genres: Vec<String>,
}

/// Separator padding lets aliases match whole words, including names like Kick04.
pub(crate) fn tokens(value: &str) -> String {
    let mut words = String::from(" ");
    let mut previous = None;
    for c in key(value).chars() {
        if c.is_alphanumeric() {
            let numeric = c.is_ascii_digit();
            if previous.is_some_and(|last| last != numeric) {
                words.push(' ');
            }
            words.push(c);
            previous = Some(numeric);
        } else if previous.take().is_some() {
            words.push(' ');
        }
    }
    if previous.is_some() {
        words.push(' ');
    }
    words
}

pub(crate) fn found(words: &str, alias: &str) -> bool {
    words.match_indices(alias).any(|(index, _)| {
        words.as_bytes().get(index.wrapping_sub(1)) == Some(&b' ')
            && words.as_bytes().get(index + alias.len()) == Some(&b' ')
    })
}

fn matching<'a>(
    words: &'a str,
    rules: &'static [(&'static str, &'static str)],
) -> impl Iterator<Item = &'static str> + 'a {
    rules
        .iter()
        .filter(move |(_, aliases)| aliases.split('|').any(|alias| found(words, alias)))
        .map(|(label, _)| *label)
}

fn unique(mut matches: impl Iterator<Item = &'static str>) -> Option<&'static str> {
    let first = matches.next()?;
    Some(if matches.any(|label| label != first) {
        UNKNOWN
    } else {
        first
    })
}

fn specific_drum(words: &str) -> Option<&'static str> {
    let rimshot = matching(words, DRUMS).any(|label| label == "림샷");
    unique(matching(words, DRUMS).filter(|label| !(rimshot && *label == "스네어")))
}

fn genre(words: &str, explicit: bool) -> Option<&'static str> {
    let aliases = if explicit { EXPLICIT_GENRES } else { &[] };
    unique(matching(words, GENRES).chain(matching(words, aliases)))
}

fn folder_genre(words: &str) -> Option<&'static str> {
    genre(words, false).or_else(|| {
        // Short style names also describe sound or texture. Accept them as whole folder names.
        let folder = words.trim();
        EXPLICIT_GENRES.iter().find_map(|(label, aliases)| {
            aliases
                .split('|')
                .any(|alias| alias == folder)
                .then_some(*label)
        })
    })
}

pub(crate) fn categories(path: &Path, root: &Path, embedded: &MetadataWords) -> Vec<String> {
    let file = tokens(&name(path));
    let metadata = tokens(&embedded.text.join(" "));
    let explicit_instrument = tokens(&embedded.instruments.join(" "));
    let explicit_genre = tokens(&embedded.genres.join(" "));
    let folders: Vec<_> = path
        .ancestors()
        .skip(1)
        .take_while(|folder| folder.starts_with(root))
        .map(|folder| tokens(&name(folder)))
        .collect();

    let mut broad_instrument = None;
    let instrument = [&explicit_instrument, &file, &metadata]
        .into_iter()
        .chain(&folders)
        .find_map(|words| {
            specific_drum(words).or_else(|| {
                let label = unique(matching(words, INSTRUMENTS))?;
                if matches!(label, "하이햇" | "심벌" | "퍼커션" | "드럼") {
                    broad_instrument.get_or_insert(label);
                    None
                } else {
                    Some(label)
                }
            })
        })
        .or(broad_instrument)
        .unwrap_or(UNKNOWN);
    let genre = genre(&explicit_genre, true)
        .or_else(|| genre(&file, false))
        .or_else(|| folders.iter().find_map(|words| folder_genre(words)))
        .unwrap_or(UNKNOWN);

    vec![format!("악기 · {instrument}"), format!("장르 · {genre}")]
}
