"""Writes outline.pdf: three pages, bookmarks (one nested), page labels
(i, then 1, 2) and two links on page 1 — one to page 3, one out to a web
address. Hand-written so the fixture is small and every byte is known.

    python3 tests/ui/fixtures/make-outline-pdf.py
"""
import pathlib

def stream(text):
    data = text.encode()
    return b"<< /Length %d >>\nstream\n" % len(data) + data + b"\nendstream"

pages = ["Preface to the work", "Chapter one begins here", "Appendix with the tables"]
objects = {}
objects[1] = b"<< /Type /Catalog /Pages 2 0 R /Outlines 20 0 R /PageLabels << /Nums [0 << /S /r >> 1 << /S /D >>] >> /PageMode /UseOutlines >>"
objects[2] = b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>"
objects[9] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"
for i, text in enumerate(pages):
    page, content = 3 + i, 10 + i
    annots = b" /Annots [30 0 R 31 0 R]" if i == 0 else b""
    objects[page] = b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] /Resources << /Font << /F1 9 0 R >> >> /Contents %d 0 R%s >>" % (content, annots)
    objects[content] = stream("BT /F1 16 Tf 30 340 Td (%s) Tj ET BT /F1 12 Tf 30 300 Td (Go to the appendix) Tj ET BT /F1 12 Tf 30 270 Td (Proton site) Tj ET" % text if i == 0 else "BT /F1 16 Tf 30 340 Td (%s) Tj ET" % text)
# Bookmarks: Preface; Chapter one > Appendix
objects[20] = b"<< /Type /Outlines /First 21 0 R /Last 22 0 R /Count 3 >>"
objects[21] = b"<< /Title (Preface) /Parent 20 0 R /Next 22 0 R /Dest [3 0 R /XYZ 0 400 null] >>"
objects[22] = b"<< /Title (Chapter one) /Parent 20 0 R /Prev 21 0 R /First 23 0 R /Last 23 0 R /Count 1 /Dest [4 0 R /XYZ 0 400 null] >>"
objects[23] = b"<< /Title (Appendix) /Parent 22 0 R /Dest [5 0 R /XYZ 0 200 null] >>"
# Links on page 1
objects[30] = b"<< /Type /Annot /Subtype /Link /Rect [28 296 160 314] /Border [0 0 0] /Dest [5 0 R /XYZ 0 400 null] >>"
objects[31] = b"<< /Type /Annot /Subtype /Link /Rect [28 266 120 284] /Border [0 0 0] /A << /S /URI /URI (https://example.org/proton) >> >>"

out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
offsets = {}
for n in sorted(objects):
    offsets[n] = len(out)
    out += b"%d 0 obj\n" % n + objects[n] + b"\nendobj\n"
size = max(objects) + 1
xref = len(out)
out += b"xref\n0 %d\n0000000000 65535 f \n" % size
for n in range(1, size):
    out += (b"%010d 00000 n \n" % offsets[n]) if n in offsets else b"0000000000 65535 f \n"
out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (size, xref)
pathlib.Path(__file__).with_name("outline.pdf").write_bytes(bytes(out))
print(len(out), "bytes")
