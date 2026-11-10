//! The deterministic citation parser from Electron's scripture detector.

use std::{borrow::Cow, sync::LazyLock};

use fancy_regex::{Captures, Regex};
use kairo_domain::js::{is_whitespace, string_to_number};

use crate::verse_bounds::is_valid_scripture_reference;

/// One citation pattern compiled twice. JavaScript's `\b` is ASCII-only;
/// spelled with lookaround it forces fancy-regex's backtracking engine. On
/// ASCII text Unicode `\b` gives the same answer, so ASCII text (nearly every
/// transcript) uses that compile and only other text pays for lookaround.
struct Pattern {
    ascii: Regex,
    any: Regex,
}

impl Pattern {
    fn new(source: &str) -> Self {
        let base = source
            .to_ascii_lowercase()
            .replace(r"\d", "[0-9]")
            .replace(
                r"\s",
                r"(?u:[\x09-\x0D\x20\u{00A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}])",
            );
        let ascii_boundary = base.replace(
            r"\b",
            r"(?:(?<![a-z0-9_])(?=[a-z0-9_])|(?<=[a-z0-9_])(?![a-z0-9_]))",
        );
        Self {
            ascii: Regex::new(&base).expect("valid citation pattern"),
            any: Regex::new(&ascii_boundary).expect("valid citation pattern"),
        }
    }

    fn on(&self, text: &str) -> &Regex {
        if text.is_ascii() {
            &self.ascii
        } else {
            &self.any
        }
    }

    fn try_replacen<'t, R: fancy_regex::Replacer>(
        &self,
        text: &'t str,
        limit: usize,
        rep: R,
    ) -> fancy_regex::Result<Cow<'t, str>> {
        self.on(text).try_replacen(text, limit, rep)
    }

    fn is_match(&self, text: &str) -> fancy_regex::Result<bool> {
        self.on(text).is_match(text)
    }

    fn find<'t>(&self, text: &'t str) -> fancy_regex::Result<Option<fancy_regex::Match<'t>>> {
        self.on(text).find(text)
    }

    fn captures<'t>(&self, text: &'t str) -> fancy_regex::Result<Option<Captures<'t, str>>> {
        self.on(text).captures(text)
    }

    fn captures_from_pos<'t>(
        &self,
        text: &'t str,
        pos: usize,
    ) -> fancy_regex::Result<Option<Captures<'t, str>>> {
        self.on(text).captures_from_pos(text, pos)
    }
}

macro_rules! pattern {
    ($pattern:expr) => {
        LazyLock::new(|| Pattern::new($pattern))
    };
}
const CANONICAL_BOOKS: &[(&str, &str)] = &[
    ("genesis", "Genesis"),
    ("gen", "Genesis"),
    ("gn", "Genesis"),
    ("exodus", "Exodus"),
    ("exod", "Exodus"),
    ("ex", "Exodus"),
    ("leviticus", "Leviticus"),
    ("lev", "Leviticus"),
    ("numbers", "Numbers"),
    ("num", "Numbers"),
    ("nu", "Numbers"),
    ("deuteronomy", "Deuteronomy"),
    ("deut", "Deuteronomy"),
    ("dt", "Deuteronomy"),
    ("joshua", "Joshua"),
    ("josh", "Joshua"),
    ("judges", "Judges"),
    ("judg", "Judges"),
    ("jdg", "Judges"),
    ("ruth", "Ruth"),
    ("1 samuel", "1 Samuel"),
    ("1sam", "1 Samuel"),
    ("1sa", "1 Samuel"),
    ("1 sam", "1 Samuel"),
    ("2 samuel", "2 Samuel"),
    ("2sam", "2 Samuel"),
    ("2sa", "2 Samuel"),
    ("2 sam", "2 Samuel"),
    ("1 kings", "1 Kings"),
    ("1kgs", "1 Kings"),
    ("1ki", "1 Kings"),
    ("1 king", "1 Kings"),
    ("2 kings", "2 Kings"),
    ("2kgs", "2 Kings"),
    ("2ki", "2 Kings"),
    ("2 king", "2 Kings"),
    ("1 chronicles", "1 Chronicles"),
    ("1chr", "1 Chronicles"),
    ("1 chr", "1 Chronicles"),
    ("2 chronicles", "2 Chronicles"),
    ("2chr", "2 Chronicles"),
    ("2 chr", "2 Chronicles"),
    ("ezra", "Ezra"),
    ("nehemiah", "Nehemiah"),
    ("neh", "Nehemiah"),
    ("esther", "Esther"),
    ("esth", "Esther"),
    ("job", "Job"),
    ("psalms", "Psalms"),
    ("psalm", "Psalms"),
    ("ps", "Psalms"),
    ("psa", "Psalms"),
    ("proverbs", "Proverbs"),
    ("prov", "Proverbs"),
    ("pro", "Proverbs"),
    ("prv", "Proverbs"),
    ("ecclesiastes", "Ecclesiastes"),
    ("eccl", "Ecclesiastes"),
    ("ecc", "Ecclesiastes"),
    ("song of solomon", "Song of Solomon"),
    ("song", "Song of Solomon"),
    ("sos", "Song of Solomon"),
    ("isaiah", "Isaiah"),
    ("isa", "Isaiah"),
    ("jeremiah", "Jeremiah"),
    ("jer", "Jeremiah"),
    ("lamentations", "Lamentations"),
    ("lam", "Lamentations"),
    ("ezekiel", "Ezekiel"),
    ("ezek", "Ezekiel"),
    ("eze", "Ezekiel"),
    ("daniel", "Daniel"),
    ("dan", "Daniel"),
    ("hosea", "Hosea"),
    ("hos", "Hosea"),
    ("joel", "Joel"),
    ("amos", "Amos"),
    ("obadiah", "Obadiah"),
    ("obad", "Obadiah"),
    ("ob", "Obadiah"),
    ("jonah", "Jonah"),
    ("jon", "Jonah"),
    ("micah", "Micah"),
    ("mic", "Micah"),
    ("nahum", "Nahum"),
    ("nah", "Nahum"),
    ("habakkuk", "Habakkuk"),
    ("hab", "Habakkuk"),
    ("zephaniah", "Zephaniah"),
    ("zeph", "Zephaniah"),
    ("zep", "Zephaniah"),
    ("haggai", "Haggai"),
    ("hag", "Haggai"),
    ("zechariah", "Zechariah"),
    ("zech", "Zechariah"),
    ("zec", "Zechariah"),
    ("malachi", "Malachi"),
    ("mal", "Malachi"),
    ("matthew", "Matthew"),
    ("matt", "Matthew"),
    ("mt", "Matthew"),
    ("mark", "Mark"),
    ("mk", "Mark"),
    ("mrk", "Mark"),
    ("luke", "Luke"),
    ("lk", "Luke"),
    ("luk", "Luke"),
    ("john", "John"),
    ("jn", "John"),
    ("jhn", "John"),
    ("acts", "Acts"),
    ("romans", "Romans"),
    ("rom", "Romans"),
    ("1 corinthians", "1 Corinthians"),
    ("1cor", "1 Corinthians"),
    ("1co", "1 Corinthians"),
    ("1 cor", "1 Corinthians"),
    ("2 corinthians", "2 Corinthians"),
    ("2cor", "2 Corinthians"),
    ("2co", "2 Corinthians"),
    ("2 cor", "2 Corinthians"),
    ("galatians", "Galatians"),
    ("gal", "Galatians"),
    ("ephesians", "Ephesians"),
    ("eph", "Ephesians"),
    ("philippians", "Philippians"),
    ("phil", "Philippians"),
    ("php", "Philippians"),
    ("colossians", "Colossians"),
    ("col", "Colossians"),
    ("1 thessalonians", "1 Thessalonians"),
    ("1thess", "1 Thessalonians"),
    ("1th", "1 Thessalonians"),
    ("1 thess", "1 Thessalonians"),
    ("2 thessalonians", "2 Thessalonians"),
    ("2thess", "2 Thessalonians"),
    ("2th", "2 Thessalonians"),
    ("2 thess", "2 Thessalonians"),
    ("1 timothy", "1 Timothy"),
    ("1tim", "1 Timothy"),
    ("1ti", "1 Timothy"),
    ("1 tim", "1 Timothy"),
    ("2 timothy", "2 Timothy"),
    ("2tim", "2 Timothy"),
    ("2ti", "2 Timothy"),
    ("2 tim", "2 Timothy"),
    ("titus", "Titus"),
    ("tit", "Titus"),
    ("philemon", "Philemon"),
    ("phlm", "Philemon"),
    ("phm", "Philemon"),
    ("hebrews", "Hebrews"),
    ("heb", "Hebrews"),
    ("james", "James"),
    ("jas", "James"),
    ("1 peter", "1 Peter"),
    ("1pet", "1 Peter"),
    ("1pe", "1 Peter"),
    ("1 pet", "1 Peter"),
    ("2 peter", "2 Peter"),
    ("2pet", "2 Peter"),
    ("2pe", "2 Peter"),
    ("2 pet", "2 Peter"),
    ("1 john", "1 John"),
    ("1jn", "1 John"),
    ("2 john", "2 John"),
    ("2jn", "2 John"),
    ("3 john", "3 John"),
    ("3jn", "3 John"),
    ("jude", "Jude"),
    ("revelation", "Revelation"),
    ("rev", "Revelation"),
    ("rv", "Revelation"),
    ("revelations", "Revelation"),
];
const NUMBER_WORDS: &[(&str, u32)] = &[
    ("one", 1),
    ("two", 2),
    ("three", 3),
    ("four", 4),
    ("five", 5),
    ("six", 6),
    ("seven", 7),
    ("eight", 8),
    ("nine", 9),
    ("ten", 10),
    ("eleven", 11),
    ("twelve", 12),
    ("thirteen", 13),
    ("fourteen", 14),
    ("fifteen", 15),
    ("sixteen", 16),
    ("seventeen", 17),
    ("eighteen", 18),
    ("nineteen", 19),
    ("twenty", 20),
    ("thirty", 30),
    ("forty", 40),
    ("fifty", 50),
    ("sixty", 60),
    ("seventy", 70),
    ("eighty", 80),
    ("ninety", 90),
];
static CORRUPTED_CITATION_RE: LazyLock<Pattern> = pattern!(
    r"(?:\b(?:song of solomon|1 thessalonians|2 thessalonians|1 corinthians|2 corinthians|1 chronicles|2 chronicles|ecclesiastes|lamentations|deuteronomy|philippians|revelations|colossians|revelation|leviticus|zephaniah|zechariah|galatians|ephesians|1 timothy|2 timothy|1 samuel|2 samuel|nehemiah|proverbs|jeremiah|habakkuk|philemon|genesis|numbers|1 kings|2 kings|ezekiel|obadiah|malachi|matthew|1 thess|2 thess|hebrews|1 peter|2 peter|exodus|joshua|judges|1 king|2 king|esther|psalms|isaiah|daniel|haggai|romans|1thess|2thess|1 john|2 john|3 john|1 sam|2 sam|1 chr|2 chr|psalm|hosea|jonah|micah|nahum|1 cor|2 cor|1 tim|2 tim|titus|james|1 pet|2 pet|exod|deut|josh|judg|ruth|1sam|2sam|1kgs|2kgs|1chr|2chr|ezra|esth|prov|eccl|song|ezek|joel|amos|obad|zeph|zech|matt|mark|luke|john|acts|1cor|2cor|phil|1tim|2tim|phlm|1pet|2pet|jude|gen|lev|num|jdg|1sa|2sa|1ki|2ki|neh|job|psa|pro|prv|ecc|sos|isa|jer|lam|eze|dan|hos|jon|mic|nah|hab|zep|hag|zec|mal|mrk|luk|jhn|rom|1co|2co|gal|eph|php|col|1th|2th|1ti|2ti|tit|phm|heb|jas|1pe|2pe|1jn|2jn|3jn|rev|gn|ex|nu|dt|ps|ob|mt|mk|lk|jn|rv)\b\s+(?:chapter\s+)?(?=[a-z0-9]*[a-z])(?=[a-z0-9]*\d)[a-z0-9]+\b)"
);
static CITATION_RE: LazyLock<Pattern> = pattern!(
    r"(?:\b((?:Gen(?:esis)?|Exo(?:dus)?|Lev(?:iticus)?|Num(?:bers)?|Deut(?:eronomy)?|Josh(?:ua)?|Judg(?:es)?|Ruth|1\s*Sam(?:uel)?|2\s*Sam(?:uel)?|1\s*Kings?|2\s*Kings?|1\s*Chr(?:onicles)?|2\s*Chr(?:onicles)?|Ezra|Neh(?:emiah)?|Esth(?:er)?|Job|Psa?(?:lms?)?|Prov(?:erbs)?|Eccl(?:esiastes)?|Song(?:\s+of\s+Solomon)?|Isa(?:iah)?|Jer(?:emiah)?|Lam(?:entations)?|Eze(?:kiel)?|Dan(?:iel)?|Hos(?:ea)?|Joel|Amos|Obad(?:iah)?|Jon(?:ah)?|Mic(?:ah)?|Nah(?:um)?|Hab(?:akkuk)?|Zeph(?:aniah)?|Hag(?:gai)?|Zech(?:ariah)?|Mal(?:achi)?|Matt(?:hew)?|Mark|Luke|John|Acts|Rom(?:ans)?|1\s*Cor(?:inthians)?|2\s*Cor(?:inthians)?|Gal(?:atians)?|Eph(?:esians)?|Phil(?:ippians)?|Col(?:ossians)?|1\s*Thess?(?:alonians)?|2\s*Thess?(?:alonians)?|1\s*Tim(?:othy)?|2\s*Tim(?:othy)?|Tit(?:us)?|Phlm|Philemon|Heb(?:rews)?|Jas|James|1\s*Pet(?:er)?|2\s*Pet(?:er)?|1\s*Jn|1\s*John|2\s*Jn|2\s*John|3\s*Jn|3\s*John|Jude|Rev(?:elation)?s?))\b(?:,(?=\s*(?:chapter\s+)?\d+\s*[:,\s]\s*(?:verses?\s+)?\d+))?\s*(?:chapter\s+)?(\d+)\b(?:\s+\2(?=\s+(?:verses?\s+)?\d+\s*(?:-|thru|through|to|and)))?(?:\s*[:,\s]\s*(?:verses?\s+)?(\d+)(?:(\s*,\s*|\s*(?:-|thru|through|to|and)\s*|\s+)(\d+))?)?)"
);
static SINGLE_NUMBER_WORD_RE: LazyLock<Pattern> = pattern!(
    r"(?:\b(?:one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety)\b)"
);
static VERSE_LEAD_IN_RE: LazyLock<Pattern> = pattern!(
    r"(?:(?:[,.;]\s*|\s+)(?:(?:and|so|now|okay|ok),?\s+)?(?:(?:let's|let\s+us|we(?:'ll|\s+will|\s+shall|\s+are\s+going\s+to)?|i\s+want\s+us\s+to)\s+)?(?:(?:begin|start)(?:ning|ing)?\s+(?:to\s+)?)?(?:(?:look(?:ing)?\s+at|read(?:ing)?|go(?:ing)?\s+to|turn(?:ing)?\s+to|see)\s+)?(?:(?:the\s+bible|it|he|she|paul|jesus|scripture)\s+(?:says|said)\s+)?(?:(?:at|from|with|in|on)\s+)?(?=verses?\s+\d))"
);
static VERSUS_BETWEEN_NUMBERS_RE: LazyLock<Pattern> = pattern!(r"(?:(\d+[,.]?\s+)versus(?=\s+\d))");
static DICTATED_CITATION_RE: LazyLock<Pattern> = pattern!(
    r"(?:\b((?:Gen(?:esis)?|Exo(?:dus)?|Lev(?:iticus)?|Num(?:bers)?|Deut(?:eronomy)?|Josh(?:ua)?|Judg(?:es)?|Ruth|1\s*Sam(?:uel)?|2\s*Sam(?:uel)?|1\s*Kings?|2\s*Kings?|1\s*Chr(?:onicles)?|2\s*Chr(?:onicles)?|Ezra|Neh(?:emiah)?|Esth(?:er)?|Job|Psa?(?:lms?)?|Prov(?:erbs)?|Eccl(?:esiastes)?|Song(?:\s+of\s+Solomon)?|Isa(?:iah)?|Jer(?:emiah)?|Lam(?:entations)?|Eze(?:kiel)?|Dan(?:iel)?|Hos(?:ea)?|Joel|Amos|Obad(?:iah)?|Jon(?:ah)?|Mic(?:ah)?|Nah(?:um)?|Hab(?:akkuk)?|Zeph(?:aniah)?|Hag(?:gai)?|Zech(?:ariah)?|Mal(?:achi)?|Matt(?:hew)?|Mark|Luke|John|Acts|Rom(?:ans)?|1\s*Cor(?:inthians)?|2\s*Cor(?:inthians)?|Gal(?:atians)?|Eph(?:esians)?|Phil(?:ippians)?|Col(?:ossians)?|1\s*Thess?(?:alonians)?|2\s*Thess?(?:alonians)?|1\s*Tim(?:othy)?|2\s*Tim(?:othy)?|Tit(?:us)?|Phlm|Philemon|Heb(?:rews)?|Jas|James|1\s*Pet(?:er)?|2\s*Pet(?:er)?|1\s*Jn|1\s*John|2\s*Jn|2\s*John|3\s*Jn|3\s*John|Jude|Rev(?:elation)?s?))\.\s+(\d+)\.?\s+(\d+)\b)"
);
static SPOKEN_CHAPTER_VERSE_RE: LazyLock<Pattern> = pattern!(
    r"(?:\b((?:Gen(?:esis)?|Exo(?:dus)?|Lev(?:iticus)?|Num(?:bers)?|Deut(?:eronomy)?|Josh(?:ua)?|Judg(?:es)?|Ruth|1\s*Sam(?:uel)?|2\s*Sam(?:uel)?|1\s*Kings?|2\s*Kings?|1\s*Chr(?:onicles)?|2\s*Chr(?:onicles)?|Ezra|Neh(?:emiah)?|Esth(?:er)?|Job|Psa?(?:lms?)?|Prov(?:erbs)?|Eccl(?:esiastes)?|Song(?:\s+of\s+Solomon)?|Isa(?:iah)?|Jer(?:emiah)?|Lam(?:entations)?|Eze(?:kiel)?|Dan(?:iel)?|Hos(?:ea)?|Joel|Amos|Obad(?:iah)?|Jon(?:ah)?|Mic(?:ah)?|Nah(?:um)?|Hab(?:akkuk)?|Zeph(?:aniah)?|Hag(?:gai)?|Zech(?:ariah)?|Mal(?:achi)?|Matt(?:hew)?|Mark|Luke|John|Acts|Rom(?:ans)?|1\s*Cor(?:inthians)?|2\s*Cor(?:inthians)?|Gal(?:atians)?|Eph(?:esians)?|Phil(?:ippians)?|Col(?:ossians)?|1\s*Thess?(?:alonians)?|2\s*Thess?(?:alonians)?|1\s*Tim(?:othy)?|2\s*Tim(?:othy)?|Tit(?:us)?|Phlm|Philemon|Heb(?:rews)?|Jas|James|1\s*Pet(?:er)?|2\s*Pet(?:er)?|1\s*Jn|1\s*John|2\s*Jn|2\s*John|3\s*Jn|3\s*John|Jude|Rev(?:elation)?s?))\s+(?:chapter\s+)?(twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety)(?:-|\s)(one|two|three|four|five|six|seven|eight|nine)\b(?!\s+(?:verses?\b|\d|(?:one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety)\b)))"
);
static VERSE_OF_BOOK_RE: LazyLock<Pattern> = pattern!(
    r"(?:\bverses?\s+(\d+(?:\s*(?:-|to|through|thru|and)\s*\d+)?)\s+(?:of|in|from)\s+(?:the\s+book\s+of\s+)?((?:Gen(?:esis)?|Exo(?:dus)?|Lev(?:iticus)?|Num(?:bers)?|Deut(?:eronomy)?|Josh(?:ua)?|Judg(?:es)?|Ruth|1\s*Sam(?:uel)?|2\s*Sam(?:uel)?|1\s*Kings?|2\s*Kings?|1\s*Chr(?:onicles)?|2\s*Chr(?:onicles)?|Ezra|Neh(?:emiah)?|Esth(?:er)?|Job|Psa?(?:lms?)?|Prov(?:erbs)?|Eccl(?:esiastes)?|Song(?:\s+of\s+Solomon)?|Isa(?:iah)?|Jer(?:emiah)?|Lam(?:entations)?|Eze(?:kiel)?|Dan(?:iel)?|Hos(?:ea)?|Joel|Amos|Obad(?:iah)?|Jon(?:ah)?|Mic(?:ah)?|Nah(?:um)?|Hab(?:akkuk)?|Zeph(?:aniah)?|Hag(?:gai)?|Zech(?:ariah)?|Mal(?:achi)?|Matt(?:hew)?|Mark|Luke|John|Acts|Rom(?:ans)?|1\s*Cor(?:inthians)?|2\s*Cor(?:inthians)?|Gal(?:atians)?|Eph(?:esians)?|Phil(?:ippians)?|Col(?:ossians)?|1\s*Thess?(?:alonians)?|2\s*Thess?(?:alonians)?|1\s*Tim(?:othy)?|2\s*Tim(?:othy)?|Tit(?:us)?|Phlm|Philemon|Heb(?:rews)?|Jas|James|1\s*Pet(?:er)?|2\s*Pet(?:er)?|1\s*Jn|1\s*John|2\s*Jn|2\s*John|3\s*Jn|3\s*John|Jude|Rev(?:elation)?s?))\s+(?:chapter\s+)?(\d+)\b)"
);
static FILLER_WORD_RE: LazyLock<Pattern> = pattern!(r"(?:[,.]?\s*\b(?:uh|um|uhm|erm|er)\b[,.]?)");
static POSSESSIVE_BEFORE_NUMBER_RE: LazyLock<Pattern> = pattern!(
    r"(?:['’]s(?=\s+(?:chapter\s+)?(?:\d+|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety)\b))"
);
static PHILIPPIANS_MISSPELLING_RE: LazyLock<Pattern> =
    pattern!(r"(?:\b(?:phill?ipp?ians?|filipp?ians?)\b)");
static SONG_OF_SONGS_RE: LazyLock<Pattern> = pattern!(r"(?:\bsongs?\s+of\s+(?:songs|solomon)\b)");
static PSALMS_MISSPELLING_RE: LazyLock<Pattern> = pattern!(
    r"(?:\b(?:pslams?|salms?(?=\s+(?:chapter\s+)?(?:\d+|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety)\b)))"
);
static PHILIPPINES_RE: LazyLock<Pattern> = pattern!(
    r"(?:\bphilippines(?=\s+(?:chapter\s+)?(?:\d+|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety)\b))"
);
static SINGULAR_BOOK_RE: LazyLock<Pattern> = pattern!(
    r"(?:\b(roman|hebrew|galatian|ephesian|colossian|corinthian|thessalonian)(?=\s+(?:chapter\s+)?(?:\d+|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety)\b))"
);
static ORDINAL_PREFIX_RE: LazyLock<Pattern> = pattern!(
    r"(?:\b(first|second|third|1st|2nd|3rd)\s+(?=(?:samuel|kings?|chronicles|corinthians|thessalonians|timothy|peter|john)\b))"
);
static ROMAN_PREFIX_RE: LazyLock<Pattern> = pattern!(
    r"(?:\b(i{1,3})\s+(?=(?:samuel|kings?|chronicles|corinthians|thessalonians|timothy|peter|john)(?=\s+(?:chapter\s+)?(?:\d+|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety)\b)))"
);

static DASH: LazyLock<Pattern> = pattern!(r"[–—]");
static NUMBER_CUE: LazyLock<Pattern> = pattern!(r"(?:\b(chapter|verses?)\s+number\s+)");
static ACTS: LazyLock<Pattern> = pattern!(r"(?:\bacts\s+of\s+the\s+apostles?\b)");
static MIXED_DIGIT_FIRST: LazyLock<Pattern> =
    pattern!(r"(?:\b([2-9])(one|two|three|four|five|six|seven|eight|nine)\b)");
static MIXED_WORD_FIRST: LazyLock<Pattern> =
    pattern!(r"(?:\b(twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety)([1-9])\b)");
static PSALM_HUNDREDS: LazyLock<Pattern> = pattern!(
    r"(?:\b(psalms?)\s+one(?:\s+hundred(?:\s+and)?)?\s+(?:(ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen)|(twenty|thirty|forty|fifty)(?:(?:-|\s)(one|two|three|four|five|six|seven|eight|nine))?)\b)"
);
static COMPOUND: LazyLock<Pattern> = pattern!(
    r"(?:\b(twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety)(?:-|\s)(one|two|three|four|five|six|seven|eight|nine)\b)"
);
static HUNDREDS: LazyLock<Pattern> = pattern!(r"(?:\b([1-9])\s+hundred(?:\s+and)?\s+(\d{1,2})\b)");
static ROUND_HUNDREDS: LazyLock<Pattern> = pattern!(r"(?:\b([1-9])\s+hundred\b)");
static ORDINAL_VERSE: LazyLock<Pattern> = pattern!(r"(?:\bthe\s+(\d+)(?:st|nd|rd|th)\s+verse\b)");
static SINGLE_CHAPTER: LazyLock<Pattern> =
    pattern!(r"(?:\b(jude|obadiah|philemon|2 john|3 john)\s+(?=verses?\s+\d+))");
static CORRECTION: LazyLock<Pattern> = pattern!(
    r"(?:(\bverse\s+|:\s*)\d+\s*[,.;-]?\s*(?:sorry|i mean|rather|make that)\s*[,.;-]?\s*(?:verse\s+)?(\d+))"
);
static VERSE_CUE: LazyLock<Pattern> = pattern!(r"(?:\bverses?\s+)");
static VERSE_LIST: LazyLock<Pattern> =
    pattern!(r"(?:^verses?\s+(\d+(?:(?:,\s*(?:and\s+)?|\s+and\s+)\d+)+))");
static DIGITS: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"[0-9]+").expect("valid digit pattern"));

fn number_word(word: &str) -> u32 {
    NUMBER_WORDS
        .iter()
        .find_map(|&(name, value)| (name == word).then_some(value))
        .expect("matched number word")
}

fn digits(word: &str) -> u32 {
    word.parse().expect("matched bounded digits")
}

pub fn canonical_book(raw: &str) -> Option<&'static str> {
    let spaced = raw
        .split(is_whitespace)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let compact = spaced.replace(' ', "");
    [spaced, compact].iter().find_map(|spelling| {
        CANONICAL_BOOKS
            .iter()
            .find_map(|&(name, book)| (name == spelling).then_some(book))
    })
}

fn valid(book: &str, chapter: u32, verse: u32) -> bool {
    is_valid_scripture_reference(book, chapter.into(), verse.into(), None)
}

pub(crate) fn normalize(text: &str) -> String {
    let mut text = text.to_lowercase();
    macro_rules! replace {
        ($pattern:ident, $replacement:expr) => {
            match $pattern.try_replacen(&text, 0, $replacement) {
                Ok(Cow::Owned(normalized)) => text = normalized,
                Ok(Cow::Borrowed(_)) => {}
                Err(error) => tracing::debug!(%error, rule = stringify!($pattern), "citation normalization failed"),
            }
        };
    }
    replace!(DASH, "-");
    replace!(FILLER_WORD_RE, "");
    replace!(POSSESSIVE_BEFORE_NUMBER_RE, "");
    replace!(PHILIPPIANS_MISSPELLING_RE, "philippians");
    replace!(SONG_OF_SONGS_RE, "song of solomon");
    replace!(PSALMS_MISSPELLING_RE, "psalms");
    replace!(PHILIPPINES_RE, "philippians");
    replace!(SINGULAR_BOOK_RE, "${1}s");
    replace!(ORDINAL_PREFIX_RE, |c: &Captures<'_, str>| match &c[1] {
        "first" | "1st" => "1 ",
        "second" | "2nd" => "2 ",
        _ => "3 ",
    });
    replace!(ROMAN_PREFIX_RE, |c: &Captures<'_, str>| format!(
        "{} ",
        c[1].len()
    ));
    replace!(NUMBER_CUE, "${1} ");
    replace!(ACTS, "acts");
    replace!(MIXED_DIGIT_FIRST, |c: &Captures<'_, str>| {
        (digits(&c[1]) * 10 + number_word(&c[2])).to_string()
    });
    replace!(MIXED_WORD_FIRST, |c: &Captures<'_, str>| {
        (number_word(&c[1]) + digits(&c[2])).to_string()
    });
    replace!(PSALM_HUNDREDS, |c: &Captures<'_, str>| {
        let tens = c.get(2).or_else(|| c.get(3)).expect("matched tens");
        let unit = c.get(4).map_or(0, |unit| number_word(unit.as_str()));
        format!("{} {}", &c[1], 100 + number_word(tens.as_str()) + unit)
    });
    replace!(SPOKEN_CHAPTER_VERSE_RE, |c: &Captures<'_, str>| {
        let chapter = number_word(&c[2]);
        let verse = number_word(&c[3]);
        if canonical_book(&c[1])
            .is_some_and(|book| !valid(book, chapter + verse, 1) && valid(book, chapter, verse))
        {
            format!("{} {chapter}:{verse}", &c[1])
        } else {
            c[0].to_owned()
        }
    });
    replace!(COMPOUND, |c: &Captures<'_, str>| {
        (number_word(&c[1]) + number_word(&c[2])).to_string()
    });
    replace!(SINGLE_NUMBER_WORD_RE, |c: &Captures<'_, str>| number_word(
        &c[0]
    )
    .to_string());
    replace!(HUNDREDS, |c: &Captures<'_, str>| {
        (digits(&c[1]) * 100 + digits(&c[2])).to_string()
    });
    replace!(ROUND_HUNDREDS, |c: &Captures<'_, str>| (digits(&c[1])
        * 100)
        .to_string());
    replace!(ORDINAL_VERSE, "verse ${1}");
    replace!(VERSUS_BETWEEN_NUMBERS_RE, "${1}verse");
    replace!(DICTATED_CITATION_RE, "${1} ${2}:${3}");
    replace!(VERSE_LEAD_IN_RE, " ");
    replace!(VERSE_OF_BOOK_RE, "${2} ${3} verse ${1}");
    replace!(SINGLE_CHAPTER, "${1} 1 ");
    replace!(CORRECTION, "${1}${2}");
    text
}

pub fn has_corrupted_scripture_citation(text: &str) -> bool {
    CORRUPTED_CITATION_RE
        .is_match(&normalize(text))
        .unwrap_or(false)
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExplicitCitation {
    pub book: &'static str,
    pub chapter: f64,
    pub verse_start: f64,
    pub verse_end: Option<f64>,
    pub source_text: String,
}

fn split_merged(book: &str, raw: &str) -> Option<(u32, u32)> {
    if !(3..=4).contains(&raw.len()) || valid(book, digits(raw), 1) {
        return None;
    }
    let mut readings = (1..raw.len()).filter_map(|cut| {
        let (chapter, verse) = raw.split_at(cut);
        let reading = (digits(chapter), digits(verse));
        (!verse.starts_with('0') && valid(book, reading.0, reading.1)).then_some(reading)
    });
    let first = readings.next()?;
    readings.next().is_none().then_some(first)
}

pub fn match_explicit_scriptures(text: &str, split_merged_digits: bool) -> Vec<ExplicitCitation> {
    match_normalized(&normalize(text), split_merged_digits)
}

pub(crate) fn match_normalized(
    normalized: &str,
    split_merged_digits: bool,
) -> Vec<ExplicitCitation> {
    let pattern_text = if normalized.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(normalized.to_ascii_lowercase())
    } else {
        Cow::Borrowed(normalized)
    };
    let mut results = Vec::new();
    let mut offset = 0;
    while let Ok(Some(c)) = CITATION_RE.captures_from_pos(pattern_text.as_ref(), offset) {
        let matched = c.get(0).expect("whole citation");
        offset = matched.end();
        let Some(book) = canonical_book(&c[1]) else {
            continue;
        };
        let mut chapter = string_to_number(&c[2]);
        let mut verse_start = c.get(3).map_or(1.0, |m| string_to_number(m.as_str()));
        let mut verse_end = c.get(5).map(|m| string_to_number(m.as_str()));
        let mut source_text = normalized[matched.start()..matched.end()].to_owned();
        if verse_end.is_some_and(|end| c[4].contains(',') && end != verse_start + 1.0) {
            verse_end = None;
        }
        if c.get(3).is_none()
            && split_merged_digits
            && let Some((split_chapter, split_verse)) = split_merged(book, &c[2])
        {
            chapter = split_chapter.into();
            verse_start = split_verse.into();
            let raw_book = c.get(1).expect("matched book");
            source_text = format!(
                "{} {chapter}:{verse_start}",
                &normalized[raw_book.start()..raw_book.end()]
            );
        }
        let mut reference = ExplicitCitation {
            book,
            chapter,
            verse_start,
            verse_end,
            source_text,
        };
        let list_at = VERSE_CUE.find(matched.as_str()).ok().flatten();
        if let Some(cue) = list_at
            && let Ok(Some(list)) =
                VERSE_LIST.captures(&normalized[matched.start() + cue.start()..])
        {
            offset = matched.start() + cue.start() + list[0].len();
            reference.source_text = normalized[matched.start()..offset].to_owned();
            let numbers: Vec<_> = DIGITS
                .find_iter(&list[1])
                .map(|m| string_to_number(m.as_str()))
                .collect();
            if numbers.len() == 2 && numbers[1] == numbers[0] + 1.0 {
                reference.verse_start = numbers[0];
                reference.verse_end = Some(numbers[1]);
                results.push(reference);
            } else {
                results.extend(numbers.into_iter().map(|verse_start| ExplicitCitation {
                    verse_start,
                    verse_end: None,
                    ..reference.clone()
                }));
            }
        } else {
            results.push(reference);
        }
    }
    results.retain_mut(|r| {
        let is_valid = |verse| is_valid_scripture_reference(r.book, r.chapter, verse, r.verse_end);
        if is_valid(r.verse_start) {
            return true;
        }
        if r.verse_end.is_none()
            && (30.0..=90.0).contains(&r.verse_start)
            && r.verse_start % 10.0 == 0.0
        {
            let teen = r.verse_start / 10.0 + 10.0;
            if is_valid(teen) {
                r.verse_start = teen;
                return true;
            }
        }
        false
    });
    results
}
static NAMED_CITATION_RE: LazyLock<Pattern> = pattern!(
    r"(?:\b(?:song of solomon|1 thessalonians|2 thessalonians|1 corinthians|2 corinthians|1 chronicles|2 chronicles|ecclesiastes|lamentations|deuteronomy|philippians|revelations|colossians|revelation|leviticus|zephaniah|zechariah|galatians|ephesians|1 timothy|2 timothy|1 samuel|2 samuel|nehemiah|proverbs|jeremiah|habakkuk|philemon|genesis|numbers|1 kings|2 kings|ezekiel|obadiah|malachi|matthew|1 thess|2 thess|hebrews|1 peter|2 peter|exodus|joshua|judges|1 king|2 king|esther|psalms|isaiah|daniel|haggai|romans|1thess|2thess|1 john|2 john|3 john|1 sam|2 sam|1 chr|2 chr|psalm|hosea|jonah|micah|nahum|1 cor|2 cor|1 tim|2 tim|titus|james|1 pet|2 pet|exod|deut|josh|judg|ruth|1sam|2sam|1kgs|2kgs|1chr|2chr|ezra|esth|prov|eccl|song|ezek|joel|amos|obad|zeph|zech|matt|mark|luke|john|acts|1cor|2cor|phil|1tim|2tim|phlm|1pet|2pet|jude|gen|lev|num|jdg|1sa|2sa|1ki|2ki|neh|job|psa|pro|prv|ecc|sos|isa|jer|lam|eze|dan|hos|jon|mic|nah|hab|zep|hag|zec|mal|mrk|luk|jhn|rom|1co|2co|gal|eph|php|col|1th|2th|1ti|2ti|tit|phm|heb|jas|1pe|2pe|1jn|2jn|3jn|rev|gn|ex|nu|dt|ps|ob|mt|mk|lk|jn|rv)\b\s+(?:chapter\b|\d+\b))"
);
static BOOK_ONLY_LEAD_IN: LazyLock<Pattern> = pattern!(
    r"(?:^(?:(?:and|so|now|okay|ok),?\s+)?(?:(?:let's|let\s+us|we're\s+going\s+to|we\s+are\s+going\s+to|i\s+want\s+you\s+to|please)\s+)?(?:(?:go|turn|open|flip|come)\s+(?:with\s+me\s+)?(?:(?:your|our)\s+bibles?\s+)?(?:with\s+me\s+)?(?:to|in(?:to)?)\s+)?(?:the\s+(?:book|gospel|epistle|letter)\s+of\s+)?)"
);

pub(crate) fn named_citation(text: &str) -> bool {
    NAMED_CITATION_RE.is_match(text).unwrap_or(false)
}

pub(crate) fn corrupted_normalized(text: &str) -> bool {
    CORRUPTED_CITATION_RE.is_match(text).unwrap_or(false)
}

pub(crate) fn bare_book(text: &str) -> Option<&'static str> {
    let stripped = BOOK_ONLY_LEAD_IN
        .try_replacen(text.trim_end_matches(['.', ',', '!', '?', ';', ':']), 1, "")
        .ok()?;
    canonical_book(&stripped)
}
