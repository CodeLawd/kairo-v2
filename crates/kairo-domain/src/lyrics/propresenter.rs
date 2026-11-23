//! ProPresenter 7 (`.pro`) lyric extraction.
//!
//! `.pro` files are Protobuf-encoded `rv.data.Presentation` messages whose
//! slide text elements store RTF. We walk only the wire fields needed for
//! song metadata, group/arrangement order, and slide RTF, then run the RTF
//! through [`super::rtf::rtf_to_plain_text`].

use std::collections::HashMap;

use super::js_text::trim;
use super::rtf::rtf_to_plain_text;

/// Lyrics and metadata extracted from a ProPresenter 7 `.pro` file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProPresenterSong {
    pub title: String,
    pub artist: String,
    pub copyright: Option<String>,
    pub ccli_number: Option<String>,
    /// `[Label]`-marked lyrics with blank lines separating ProPresenter slides.
    pub text: String,
}

enum WireValue<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed,
}

struct ProtoReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> ProtoReader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn read_varint(&mut self) -> Result<u64, String> {
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            let byte = *self
                .buf
                .get(self.pos)
                .ok_or_else(|| "That ProPresenter file is truncated.".to_owned())?;
            self.pos += 1;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
            if shift >= 64 {
                return Err("That file is not a valid ProPresenter presentation.".into());
            }
        }
    }

    fn next(&mut self) -> Result<Option<(u32, WireValue<'a>)>, String> {
        if self.pos >= self.buf.len() {
            return Ok(None);
        }
        let tag = self.read_varint()?;
        let field = u32::try_from(tag >> 3)
            .map_err(|_| "That file is not a valid ProPresenter presentation.".to_owned())?;
        if field == 0 {
            return Err("That file is not a valid ProPresenter presentation.".into());
        }
        let value = match tag & 0x7 {
            0 => WireValue::Varint(self.read_varint()?),
            1 => {
                self.skip(8)?;
                WireValue::Fixed
            }
            2 => {
                let len = usize::try_from(self.read_varint()?)
                    .map_err(|_| "That ProPresenter file is truncated.".to_owned())?;
                let end = self
                    .pos
                    .checked_add(len)
                    .filter(|&e| e <= self.buf.len())
                    .ok_or_else(|| "That ProPresenter file is truncated.".to_owned())?;
                let slice = &self.buf[self.pos..end];
                self.pos = end;
                WireValue::Bytes(slice)
            }
            5 => {
                self.skip(4)?;
                WireValue::Fixed
            }
            _ => return Err("That file is not a valid ProPresenter presentation.".into()),
        };
        Ok(Some((field, value)))
    }

    fn skip(&mut self, n: usize) -> Result<(), String> {
        self.pos = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.buf.len())
            .ok_or_else(|| "That ProPresenter file is truncated.".to_owned())?;
        Ok(())
    }
}

fn read_uuid(bytes: &[u8]) -> Result<String, String> {
    let mut r = ProtoReader::new(bytes);
    while let Some((field, val)) = r.next()? {
        if field == 1
            && let WireValue::Bytes(s) = val
        {
            return Ok(String::from_utf8_lossy(s).into_owned());
        }
    }
    Ok(String::new())
}

#[derive(Default)]
struct CueGroup {
    uuid: String,
    name: String,
    cue_ids: Vec<String>,
}

fn read_cue_group(bytes: &[u8]) -> Result<CueGroup, String> {
    let mut out = CueGroup::default();
    let mut r = ProtoReader::new(bytes);
    while let Some((field, val)) = r.next()? {
        match (field, val) {
            (1, WireValue::Bytes(group)) => {
                let mut gr = ProtoReader::new(group);
                while let Some((gf, gv)) = gr.next()? {
                    match (gf, gv) {
                        (1, WireValue::Bytes(u)) => out.uuid = read_uuid(u)?,
                        (2, WireValue::Bytes(n)) => {
                            out.name = trim(&String::from_utf8_lossy(n)).to_owned();
                        }
                        _ => {}
                    }
                }
            }
            (2, WireValue::Bytes(u)) => {
                let id = read_uuid(u)?;
                if !id.is_empty() {
                    out.cue_ids.push(id);
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

fn read_text_rtf(bytes: &[u8]) -> Result<Option<String>, String> {
    let mut r = ProtoReader::new(bytes);
    while let Some((field, val)) = r.next()? {
        if field == 5
            && let WireValue::Bytes(rtf) = val
        {
            let plain = rtf_to_plain_text(&String::from_utf8_lossy(rtf));
            let trimmed = trim(&plain);
            if !trimmed.is_empty() {
                return Ok(Some(trimmed.to_owned()));
            }
        }
    }
    Ok(None)
}

fn read_slide_texts(bytes: &[u8], out: &mut Vec<String>) -> Result<(), String> {
    let mut r = ProtoReader::new(bytes);
    while let Some((field, val)) = r.next()? {
        // Slide.elements (1) -> Slide.Element.element (1) -> Graphics.Element.text (13)
        if field == 1
            && let WireValue::Bytes(slide_el) = val
        {
            let mut er = ProtoReader::new(slide_el);
            while let Some((ef, ev)) = er.next()? {
                if ef == 1
                    && let WireValue::Bytes(gfx_el) = ev
                {
                    let mut gr = ProtoReader::new(gfx_el);
                    while let Some((gf, gv)) = gr.next()? {
                        if gf == 13
                            && let WireValue::Bytes(text_msg) = gv
                            && let Some(text) = read_text_rtf(text_msg)?
                        {
                            out.push(text);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn read_cue(bytes: &[u8]) -> Result<(String, Option<String>), String> {
    let mut uuid = String::new();
    let mut texts = Vec::new();
    let mut r = ProtoReader::new(bytes);
    while let Some((field, val)) = r.next()? {
        match (field, val) {
            (1, WireValue::Bytes(u)) => uuid = read_uuid(u)?,
            // Cue.actions (10) -> Action.slide (23) -> SlideType.presentation (2) -> base_slide (1)
            (10, WireValue::Bytes(action)) => {
                let mut ar = ProtoReader::new(action);
                while let Some((af, av)) = ar.next()? {
                    if af == 23
                        && let WireValue::Bytes(slide_type) = av
                    {
                        let mut sr = ProtoReader::new(slide_type);
                        while let Some((sf, sv)) = sr.next()? {
                            if sf == 2
                                && let WireValue::Bytes(pres_slide) = sv
                            {
                                let mut pr = ProtoReader::new(pres_slide);
                                while let Some((pf, pv)) = pr.next()? {
                                    if pf == 1
                                        && let WireValue::Bytes(base_slide) = pv
                                    {
                                        read_slide_texts(base_slide, &mut texts)?;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let slide = (!texts.is_empty()).then(|| texts.join("\n"));
    Ok((uuid, slide))
}

#[derive(Default)]
struct Ccli {
    author: String,
    copyright_year: u64,
    song_title: String,
    publisher: String,
    song_number: u64,
}

fn read_ccli(bytes: &[u8]) -> Result<Ccli, String> {
    let mut out = Ccli::default();
    let mut r = ProtoReader::new(bytes);
    while let Some((field, val)) = r.next()? {
        match (field, val) {
            (1, WireValue::Bytes(s)) => out.author = trim(&String::from_utf8_lossy(s)).to_owned(),
            (2, WireValue::Varint(y)) => out.copyright_year = y,
            (3, WireValue::Bytes(s)) => {
                out.song_title = trim(&String::from_utf8_lossy(s)).to_owned();
            }
            (4, WireValue::Bytes(s)) => {
                out.publisher = trim(&String::from_utf8_lossy(s)).to_owned();
            }
            (5, WireValue::Varint(n)) => out.song_number = n,
            _ => {}
        }
    }
    Ok(out)
}

#[derive(Default)]
struct Arrangement {
    uuid: String,
    group_ids: Vec<String>,
}

fn read_arrangement(bytes: &[u8]) -> Result<Arrangement, String> {
    let mut out = Arrangement::default();
    let mut r = ProtoReader::new(bytes);
    while let Some((field, val)) = r.next()? {
        match (field, val) {
            (1, WireValue::Bytes(u)) => out.uuid = read_uuid(u)?,
            (3, WireValue::Bytes(u)) => {
                let id = read_uuid(u)?;
                if !id.is_empty() {
                    out.group_ids.push(id);
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

fn sanitize_label(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|&c| c != '[' && c != ']' && c != '\r' && c != '\n')
        .collect();
    let trimmed = trim(&cleaned);
    // Keep within `section_edit::bracket_label`'s 40 UTF-16 code unit limit.
    let mut units = 0;
    let mut out = String::new();
    for ch in trimmed.chars() {
        let w = ch.len_utf16();
        if units + w > 40 {
            break;
        }
        out.push(ch);
        units += w;
    }
    trim(&out).to_owned()
}

/// Extracts lyrics and metadata from a ProPresenter 7 `.pro` binary.
pub fn parse_propresenter(bytes: &[u8]) -> Result<ProPresenterSong, String> {
    if bytes.is_empty() {
        return Err("That ProPresenter file is empty.".into());
    }

    let mut pres_name = String::new();
    let mut cue_groups = Vec::new();
    let mut cues_in_order: Vec<(String, String)> = Vec::new();
    let mut cues_by_id: HashMap<String, String> = HashMap::new();
    let mut ccli = Ccli::default();
    let mut arrangements = Vec::new();
    let mut selected_arrangement = String::new();

    let mut r = ProtoReader::new(bytes);
    while let Some((field, val)) = r.next()? {
        match (field, val) {
            (3, WireValue::Bytes(s)) => pres_name = trim(&String::from_utf8_lossy(s)).to_owned(),
            (12, WireValue::Bytes(g)) => cue_groups.push(read_cue_group(g)?),
            (13, WireValue::Bytes(c)) => {
                let (uuid, slide) = read_cue(c)?;
                if let Some(text) = slide {
                    if !uuid.is_empty() {
                        cues_by_id.insert(uuid.clone(), text.clone());
                    }
                    cues_in_order.push((uuid, text));
                }
            }
            (14, WireValue::Bytes(c)) => ccli = read_ccli(c)?,
            (15, WireValue::Bytes(a)) => arrangements.push(read_arrangement(a)?),
            (16, WireValue::Bytes(u)) => selected_arrangement = read_uuid(u)?,
            _ => {}
        }
    }

    let ordered_groups: Vec<&CueGroup> = if !selected_arrangement.is_empty() {
        let by_id: HashMap<&str, &CueGroup> =
            cue_groups.iter().map(|g| (g.uuid.as_str(), g)).collect();
        arrangements
            .iter()
            .find(|a| a.uuid == selected_arrangement && !a.group_ids.is_empty())
            .map(|a| {
                a.group_ids
                    .iter()
                    .filter_map(|id| by_id.get(id.as_str()).copied())
                    .collect::<Vec<_>>()
            })
            .filter(|groups| !groups.is_empty())
            .unwrap_or_else(|| cue_groups.iter().collect())
    } else {
        cue_groups.iter().collect()
    };

    let mut sections: Vec<String> = Vec::new();
    let mut unnamed_count = 0usize;
    for group in ordered_groups {
        let slides: Vec<&str> = group
            .cue_ids
            .iter()
            .filter_map(|id| cues_by_id.get(id).map(String::as_str))
            .collect();
        if slides.is_empty() {
            continue;
        }
        let label = sanitize_label(&group.name);
        let label = if label.is_empty() {
            unnamed_count += 1;
            format!("Verse {unnamed_count}")
        } else {
            label
        };
        sections.push(format!("[{label}]\n{}", slides.join("\n\n")));
    }

    if sections.is_empty() && !cues_in_order.is_empty() {
        let slides: Vec<&str> = cues_in_order.iter().map(|(_, t)| t.as_str()).collect();
        sections.push(format!("[Verse 1]\n{}", slides.join("\n\n")));
    }

    if sections.is_empty() {
        return Err("That ProPresenter file has no readable lyrics.".into());
    }

    let title = if !ccli.song_title.is_empty() {
        ccli.song_title
    } else {
        pres_name
    };
    let copyright = match (ccli.copyright_year > 0, !ccli.publisher.is_empty()) {
        (true, true) => Some(format!("© {} {}", ccli.copyright_year, ccli.publisher)),
        (true, false) => Some(format!("© {}", ccli.copyright_year)),
        (false, true) => Some(ccli.publisher),
        (false, false) => None,
    };
    let ccli_number = (ccli.song_number > 0).then(|| ccli.song_number.to_string());

    Ok(ProPresenterSong {
        title,
        artist: ccli.author,
        copyright,
        ccli_number,
        text: sections.join("\n\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn varint(mut v: u64, out: &mut Vec<u8>) {
        loop {
            let mut b = (v & 0x7f) as u8;
            v >>= 7;
            if v != 0 {
                b |= 0x80;
            }
            out.push(b);
            if v == 0 {
                break;
            }
        }
    }

    fn field_bytes(field: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        varint(((field as u64) << 3) | 2, &mut out);
        varint(payload.len() as u64, &mut out);
        out.extend_from_slice(payload);
        out
    }

    fn field_varint(field: u32, v: u64) -> Vec<u8> {
        let mut out = Vec::new();
        varint((field as u64) << 3, &mut out);
        varint(v, &mut out);
        out
    }

    fn uuid_msg(id: &str) -> Vec<u8> {
        field_bytes(1, id.as_bytes())
    }

    fn cue_msg(id: &str, rtf_slides: &[&str]) -> Vec<u8> {
        let mut base_slide = Vec::new();
        for rtf in rtf_slides {
            let text = field_bytes(5, rtf.as_bytes());
            let gfx_el = field_bytes(13, &text);
            let slide_el = field_bytes(1, &gfx_el);
            base_slide.extend(field_bytes(1, &slide_el));
        }
        let pres_slide = field_bytes(1, &base_slide);
        let slide_type = field_bytes(2, &pres_slide);
        let action = field_bytes(23, &slide_type);
        let mut cue = field_bytes(1, &uuid_msg(id));
        cue.extend(field_bytes(10, &action));
        cue
    }

    fn cue_group_msg(id: &str, name: &str, cue_ids: &[&str]) -> Vec<u8> {
        let mut group = field_bytes(1, &uuid_msg(id));
        if !name.is_empty() {
            group.extend(field_bytes(2, name.as_bytes()));
        }
        let mut out = field_bytes(1, &group);
        for cid in cue_ids {
            out.extend(field_bytes(2, &uuid_msg(cid)));
        }
        out
    }

    #[test]
    fn orders_slides_by_cue_groups_and_decodes_rtf() {
        // Matches Alpha_Omega-1.pro: cues stored in reverse order, cue_group puts slide 1 first.
        let mut pres = Vec::new();
        pres.extend(field_bytes(3, b" Alpha Omega-1"));
        pres.extend(field_bytes(
            12,
            &cue_group_msg("g1", "", &["c-first", "c-second"]),
        ));
        pres.extend(field_bytes(
            13,
            &cue_msg(
                "c-second",
                &[r"{\rtf1\ansi\ansicpg1252 Iwo ni mo f\'92iyin fun o Baba}"],
            ),
        ));
        pres.extend(field_bytes(
            13,
            &cue_msg("c-first", &[r"{\rtf1\ansi Alpha Omega}"]),
        ));
        pres.extend(field_bytes(14, &field_bytes(3, b"Alpha Omega")));

        let song = parse_propresenter(&pres).expect("parsed");
        assert_eq!(song.title, "Alpha Omega");
        assert_eq!(song.artist, "");
        assert_eq!(song.copyright, None);
        assert_eq!(song.ccli_number, None);
        assert_eq!(
            song.text,
            "[Verse 1]\nAlpha Omega\n\nIwo ni mo f\u{2019}iyin fun o Baba"
        );
    }

    #[test]
    fn follows_selected_arrangement_and_ccli_metadata() {
        let mut pres = Vec::new();
        pres.extend(field_bytes(3, b"Fallback Name"));
        pres.extend(field_bytes(12, &cue_group_msg("g-v1", "Verse 1", &["c1"])));
        pres.extend(field_bytes(12, &cue_group_msg("g-ch", "Chorus", &["c2"])));
        pres.extend(field_bytes(13, &cue_msg("c1", &[r"{\rtf1\ansi Line one}"])));
        pres.extend(field_bytes(
            13,
            &cue_msg("c2", &[r"{\rtf1\ansi Chorus line}"]),
        ));

        let mut ccli = Vec::new();
        ccli.extend(field_bytes(1, b"Sinach"));
        ccli.extend(field_varint(2, 2015));
        ccli.extend(field_bytes(3, b"Way Maker"));
        ccli.extend(field_bytes(4, b"Integrity Music"));
        ccli.extend(field_varint(5, 7115744));
        pres.extend(field_bytes(14, &ccli));

        let mut arr = field_bytes(1, &uuid_msg("arr-1"));
        arr.extend(field_bytes(3, &uuid_msg("g-ch")));
        arr.extend(field_bytes(3, &uuid_msg("g-v1")));
        arr.extend(field_bytes(3, &uuid_msg("g-ch")));
        pres.extend(field_bytes(15, &arr));
        pres.extend(field_bytes(16, &uuid_msg("arr-1")));

        let song = parse_propresenter(&pres).expect("parsed");
        assert_eq!(song.title, "Way Maker");
        assert_eq!(song.artist, "Sinach");
        assert_eq!(song.copyright.as_deref(), Some("© 2015 Integrity Music"));
        assert_eq!(song.ccli_number.as_deref(), Some("7115744"));
        assert_eq!(
            song.text,
            "[Chorus]\nChorus line\n\n[Verse 1]\nLine one\n\n[Chorus]\nChorus line"
        );
    }

    #[test]
    fn rejects_empty_or_corrupt_files() {
        assert!(parse_propresenter(b"").is_err());
        assert!(parse_propresenter(&[0x00]).is_err());
        assert!(parse_propresenter(&[0x0a, 0xff, 0x01]).is_err());
        assert!(parse_propresenter(&field_bytes(3, b"No Slides")).is_err());
    }
}
