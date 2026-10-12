"""Writes scan.pdf: two pages, the first only a picture drawn — lines and a
box, as a scan is to search, with no text — the second with words on it.

    python3 tests/ui/fixtures/make-scan-pdf.py
"""
import pathlib

def stream(text):
    data = text.encode()
    return b"<< /Length %d >>\nstream\n" % len(data) + data + b"\nendstream"

objects = {}
objects[1] = b"<< /Type /Catalog /Pages 2 0 R >>"
objects[2] = b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>"
objects[3] = b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] /Resources << >> /Contents 10 0 R >>"
objects[4] = b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] /Resources << /Font << /F1 9 0 R >> >> /Contents 11 0 R >>"
objects[9] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"
lines = " ".join("30 %d m 270 %d l" % (y, y) for y in range(340, 120, -24))
objects[10] = stream("0.85 g 20 60 260 320 re f 0.2 G 2 w %s S" % lines)
objects[11] = stream("BT /F1 16 Tf 30 340 Td (Typed words on page two) Tj ET")

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
pathlib.Path(__file__).with_name("scan.pdf").write_bytes(bytes(out))
print(len(out), "bytes")
