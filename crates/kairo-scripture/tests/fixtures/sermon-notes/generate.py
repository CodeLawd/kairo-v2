#!/usr/bin/env python3
"""Writes the sermon notes corpus for prompt 042's differential test.

Every sample sermon is written as DOCX, PDF, TXT, MD and RTF, plus edge-case
files. Only the standard library is used. Run from anywhere:

    python3 -I crates/kairo-scripture/tests/fixtures/sermon-notes/generate.py
"""
import os
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))

SERMONS = {
    "grace": [
        ("h1", "Saved by Grace"),
        ("p", "Opening reading: Ephesians 2:8-9 (NKJV)"),
        ("p", "Paul builds on Romans 3:23, 24 and Romans 5:8; 6:23."),
        ("li", "Titus 3:5 ESV"),
        ("li", "John 3:16, 17; John 1:12"),
        ("p", "We sing at 10:30 and pray for 5 minutes; 3 songs follow."),
        ("p", "NIV Galatians 2:20 and 2 Corinthians 5:17 (KJV, NIV)"),
    ],
    "shepherd": [
        ("h2", "The Lord Is My Shepherd"),
        ("p", "Psalm 23:1\u20136 is the text. Compare Ps 100:3 and John 10:11, 14-16."),
        ("p", "Isaiah 40:11 NLT \u2014 He tends his flock."),
        ("p", "1 Peter 5:4; 2:25 and Heb 13:20\u201421 [AMP]"),
        ("li", "Ezekiel 34:11, 12, 15"),
        ("li", "Luke 15:4-7 (NASB)"),
        ("p", "Matthew 9 36 is written without a colon."),
    ],
    "faith": [
        ("h1", "Faith That Works"),
        ("p", "Hebrews 11:1, 6 and James 2:17-26 (NIV/ESV)."),
        ("p", "Genesis 15:6; Rom 4:3, 4:20-22 and Gal 3:6."),
        ("p", "Mark 11:22 KJV, Mark 9:23 and 2 Cor 5:7"),
        ("li", "1 John 5:4"),
        ("li", "Habakkuk 2:4 and Romans 1:17; 10:17"),
        ("p", "Chapter 12 has 29 verses; read it at home."),
    ],
    "hope": [
        ("h3", "Living Hope"),
        ("p", "1 Thessalonians 4:13-18, 5:1-2 and Revelation 21:4"),
        ("p", "Jeremiah 29:11 (MSG) and Lamentations 3:22\u201323 ESV"),
        ("p", "Romans 15:13; 8:24, 25 and Colossians 1:27."),
        ("li", "Song of Solomon 2:11-12"),
        ("li", "1 Corinthians 15:51-57 and Phil 1:21, 23"),
    ],
}


def plain_lines(blocks):
    return [text for _, text in blocks]


def write(name, data):
    with open(os.path.join(HERE, name), "wb") as f:
        f.write(data)


def xml_escape(text):
    return text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def docx(blocks):
    ns = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"'
    body = []
    for kind, text in blocks:
        ppr = ""
        if kind.startswith("h"):
            ppr = '<w:pPr><w:pStyle w:val="Heading%s"/></w:pPr>' % kind[1]
        elif kind == "li":
            ppr = '<w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr>'
        # Split each paragraph into two runs, the second bold, so run joins are exercised.
        half = len(text) // 2
        runs = '<w:r><w:t xml:space="preserve">%s</w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">%s</w:t></w:r>' % (
            xml_escape(text[:half]), xml_escape(text[half:]))
        body.append("<w:p>%s%s</w:p>" % (ppr, runs))
    document = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document %s><w:body>%s<w:sectPr/></w:body></w:document>' % (ns, "".join(body))
    styles = '<?xml version="1.0" encoding="UTF-8"?><w:styles %s>%s</w:styles>' % (ns, "".join(
        '<w:style w:type="paragraph" w:styleId="Heading%d"><w:name w:val="heading %d"/></w:style>' % (i, i) for i in range(1, 7)))
    numbering = ('<?xml version="1.0" encoding="UTF-8"?><w:numbering %s><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0">'
                 '<w:numFmt w:val="bullet"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>') % ns
    return zip_docx(document, styles, numbering)


def zip_docx(document, styles=None, numbering=None):
    import io
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        def add(name, body):
            info = zipfile.ZipInfo(name, date_time=(2026, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, body)
        overrides = '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
        add("[Content_Types].xml", '<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>%s</Types>' % overrides)
        add("_rels/.rels", '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>')
        rels = []
        if styles:
            rels.append('<Relationship Id="rS" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>')
            add("word/styles.xml", styles)
        if numbering:
            rels.append('<Relationship Id="rN" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>')
            add("word/numbering.xml", numbering)
        add("word/_rels/document.xml.rels", '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">%s</Relationships>' % "".join(rels))
        add("word/document.xml", document)
    return buf.getvalue()


WINANSI = {"\u2013": 0x96, "\u2014": 0x97}


def pdf_string(text):
    out = []
    for ch in text:
        code = WINANSI.get(ch, ord(ch))
        if code > 255:
            raise ValueError(ch)
        c = chr(code)
        if c in "()\\":
            out.append("\\" + c)
        elif code < 32 or code > 126:
            out.append("\\%03o" % code)
        else:
            out.append(c)
    return "".join(out)


def pdf(pages):
    """Each page is a list of lines, set in Helvetica with WinAnsiEncoding."""
    objects = []
    def add(body):
        objects.append(body)
        return len(objects)
    font = add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>")
    pages_id = len(objects) + 1 + 2 * len(pages)
    page_ids = []
    for lines in pages:
        ops = ["BT", "/F1 12 Tf", "72 720 Td", "16 TL"]
        for i, line in enumerate(lines):
            ops.append("(%s) Tj" % pdf_string(line))
            ops.append("T*")
        ops.append("ET")
        stream = "\n".join(ops).encode("latin-1")
        content = add(b"<< /Length %d >>\nstream\n" % len(stream) + stream + b"\nendstream")
        page_ids.append(add(("<< /Type /Page /Parent %d 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R >>" % (pages_id, font, content)).encode()))
    assert add(("<< /Type /Pages /Kids [%s] /Count %d >>" % (" ".join("%d 0 R" % p for p in page_ids), len(page_ids))).encode()) == pages_id
    catalog = add(("<< /Type /Catalog /Pages %d 0 R >>" % pages_id).encode())
    out = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    # Electron's pdf-parse (pdf.js 1.10) rejects these tiny files with "bad
    # XRef entry"; real notes are far larger. Pad past 4 KB with a comment.
    out += (b"% padding" + b"." * 70 + b"\n") * 64
    offsets = []
    for i, body in enumerate(objects, 1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % i + body + b"\nendobj\n"
    xref = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objects) + 1)
    for off in offsets:
        out += b"%010d 00000 n \n" % off
    out += b"trailer\n<< /Size %d /Root %d 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objects) + 1, catalog, xref)
    return bytes(out)


def rtf(lines):
    def esc(text):
        out = []
        for ch in text:
            if ch in "\\{}":
                out.append("\\" + ch)
            elif ch in WINANSI:
                out.append("\\'%02x" % WINANSI[ch])
            elif ord(ch) > 127:
                out.append("\\u%d?" % ord(ch))
            else:
                out.append(ch)
        return "".join(out)
    body = "\\par\n".join(esc(line) for line in lines)
    return ("{\\rtf1\\ansi\\ansicpg1252\\cocoartf2822\n{\\fonttbl\\f0\\fswiss\\fcharset0 Helvetica;}\n"
            "{\\colortbl;\\red255\\green255\\blue255;}\n\\pard\\tx720\\pardirnatural\\partightenfactor0\n\n"
            "\\f0\\fs24 \\cf0 %s}" % body).encode("ascii")


def markdown(blocks):
    out = []
    for kind, text in blocks:
        if kind.startswith("h"):
            out.append("#" * int(kind[1]) + " " + text)
        elif kind == "li":
            out.append("- " + text)
        else:
            out.append(text)
    return "\n\n".join(out) + "\n"


def main():
    for name, blocks in SERMONS.items():
        lines = plain_lines(blocks)
        write(name + ".docx", docx(blocks))
        half = (len(lines) + 1) // 2
        write(name + ".pdf", pdf([lines[:half], lines[half:]]))
        write(name + ".txt", ("\n".join(lines) + "\n").encode("utf-8"))
        write(name + ".md", markdown(blocks).encode("utf-8"))
        write(name + ".rtf", rtf(lines))
    # Edge cases.
    write("empty.txt", b"")
    write("empty.docx", zip_docx('<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body/></w:document>'))
    write("blank-page.pdf", pdf([[]]))
    write("corrupt.pdf", b"%PDF-1.4\nthis is not a pdf\n")
    write("corrupt.docx", b"PK\x03\x04 not really a zip")
    write("zero-bytes.docx", b"")
    import io
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as z:
        z.writestr(zipfile.ZipInfo("readme.txt", date_time=(2026, 1, 1, 0, 0, 0)), "no document part")
    write("no-document.docx", buf.getvalue())
    write("notes.odt", b"unsupported")
    write("bom-and-invalid.txt", b"\xef\xbb\xbfJohn 3:16 \xff and Romans 8:28\xe2\x80\x9330\n")
    write("rtf-markup.txt", rtf(["Rtf saved as txt: Acts 2:38\u201339"]))
    write("SHOUTING.TXT", b"MARK 16:15 AND 1 JOHN 1:9 (KJV)\n")
    write("crlf.md", b"# Notes\r\n\r\nJohn 14:6\r\nActs 4:12, 13\r\n")


if __name__ == "__main__":
    main()
