#!/usr/bin/env python3
"""A realistic demo mailbox for jane@example.test (password janepass), used for README
screenshots: a team thread spanning Inbox and Sent, attachments, a newsletter, a mix of
read/unread/starred. Re-running replaces it.   dev/seed-demo.py
"""
import base64, os, subprocess, tarfile, tempfile, time
from email.message import EmailMessage
from email.utils import formatdate

HERE = os.path.dirname(os.path.abspath(__file__))
dc = ["docker", "compose", "-f", os.path.join(HERE, "compose.yml")]
sh = lambda cmd: subprocess.run(dc + ["exec", "-T", "mail", "sh", "-c", cmd], check=True, capture_output=True, text=True)
ME = "Jane Cooper <jane@example.test>"
now = time.time()
H = 3600

if "jane@example.test" not in sh("setup email list || true").stdout:
    sh("setup email add jane@example.test janepass")
    time.sleep(3)

# A tiny real PNG, recoloured per "photo" (1×1 pixel scaled up by the viewer is fine for a
# thumbnail strip; the reader shows previews at 110 px high).
def png(rgb):
    import struct, zlib
    w = h = 64
    raw = b"".join(b"\x00" + bytes(rgb) * w for _ in range(h))
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) + \
        chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")

def msg(frm, to, subject, body, ago, mid, refs=None, html=None, files=()):
    m = EmailMessage()
    m["From"], m["To"], m["Subject"] = frm, to, subject
    m["Date"] = formatdate(now - ago, localtime=True)
    m["Message-ID"] = f"<{mid}@demo.example>"
    if refs:
        m["In-Reply-To"] = f"<{refs[-1]}@demo.example>"
        m["References"] = " ".join(f"<{r}@demo.example>" for r in refs)
    m.set_content(body)
    if html:
        m.add_alternative(html, subtype="html")
    for name, ctype, data in files:
        maintype, subtype = ctype.split("/")
        m.add_attachment(data, maintype=maintype, subtype=subtype, filename=name)
    return m, now - ago

inbox = [
    (msg("Maria Lopez <maria@northwind.example>", ME, "Re: Design review moved to Thursday",
         "Perfect — Thursday at 14:00 it is. I'll book the big room and send the deck tonight.\n\nMaria",
         0.6 * H, "review-3", refs=["review-1", "review-2"]), ""),
    (msg("Northwind Hosting <billing@northwind-hosting.example>", ME, "Your invoice for October",
         "Hi Jane,\n\nYour invoice for October is attached. Amount due: $48.00, paid automatically on 5 November.\n\nThanks for being with us.",
         1.4 * H, "invoice", files=[("invoice-2026-10.pdf", "application/pdf",
         b"%PDF-1.4\n1 0 obj<<>>endobj\ntrailer<<>>\n%%EOF")]), ""),
    (msg("Tom Becker <tom@northwind.example>", ME, "Photos from the offsite 📸",
         "A few favourites from last week. The rest are in the shared album.\n\nTom",
         3 * H, "photos", files=[("lake.png", "image/png", png((70, 130, 180))),
         ("sunset.png", "image/png", png((240, 140, 60))), ("team.png", "image/png", png((90, 160, 110)))]), "F"),
    (msg("Alex Kim <alex@northwind.example>", ME, "Q4 budget — numbers for review",
         "Hi Jane,\n\nThe Q4 numbers are in the planning doc. Could you check the marketing line before Friday?\n\nAlex",
         5 * H, "budget"), ""),
    (msg("The Product Team <news@productweekly.example>", ME, "This week: faster search and dark mode",
         "Faster search, dark mode, and three small things we love.",
         20 * H, "weekly", html="""<div style="font-family:system-ui,sans-serif;max-width:560px">
<h1 style="color:#3b5bdb">This week in the product</h1>
<p><b>Search is 8× faster.</b> Results now appear as you type, even in folders with tens of
thousands of messages.</p><p><b>Dark mode</b> follows your system — or pick it in Settings.</p>
<p><a href="https://example.com/blog">Read the full post →</a></p></div>"""), "S"),
    (msg("Sam Rivera <sam@example.com>", ME, "Lunch on Friday?", "The new noodle place opened on 5th street. 12:30?",
         27 * H, "lunch"), "S"),
    (msg("Code Review <noreply@code.example>", ME, "[webmail] PR #482: Cache sorted UID lists",
         "maria requested your review on #482.", 30 * H, "pr482"), "S"),
    (msg("SkyLine Airways <booking@skyline.example>", ME, "Your flight confirmation — ULN → ICN",
         "Booking reference K7Q2PD. Departure 14 Oct, 08:45. Seat 14A.", 50 * H, "flight"), "FS"),
    (msg("Priya Shah <priya@legal.example>", ME, "Contract draft v3",
         "Hi Jane — v3 attached with the changes from Tuesday's call highlighted.\n\nPriya",
         74 * H, "contract", files=[("contract-v3.pdf", "application/pdf",
         b"%PDF-1.4\n1 0 obj<<>>endobj\ntrailer<<>>\n%%EOF")]), "S"),
    (msg("Maria Lopez <maria@northwind.example>", ME, "Design review moved to Thursday",
         "Hi Jane,\n\nThe design review clashes with the board call on Wednesday. Could we move it to Thursday afternoon?\n\nMaria",
         26 * H, "review-1"), "S"),
    (msg("People Team <people@northwind.example>", ME, "Welcome to the team, Jane! 🎉",
         "We're so glad you're here. Your first-week schedule is below.", 120 * H, "welcome"), "S"),
]
sent = [
    (msg(ME, "Maria Lopez <maria@northwind.example>", "Re: Design review moved to Thursday",
         "Thursday works — 14:00? I'll bring the prototype.\n\nJane", 25 * H, "review-2", refs=["review-1"]), "S"),
]

with tempfile.TemporaryDirectory() as tmp:
    for folder, items in (("", inbox), (".Sent", sent)):
        cur = os.path.join(tmp, folder, "cur")
        os.makedirs(cur, exist_ok=True)
        for i, ((m, t), flags) in enumerate(items):
            p = os.path.join(cur, f"{int(t)}.demo{folder}{i}.local:2,{flags}")
            with open(p, "wb") as f:
                f.write(bytes(m))
            os.utime(p, (t, t))
    tar = os.path.join(tmp, "demo.tar")
    with tarfile.open(tar, "w", format=tarfile.PAX_FORMAT) as tf:
        for d in ("cur", ".Sent"):
            tf.add(os.path.join(tmp, d), arcname=d)
    subprocess.run(dc + ["cp", tar, "mail:/tmp/demo.tar"], check=True, capture_output=True)
sh("doveadm expunge -u jane@example.test mailbox INBOX all || true; "
   "doveadm expunge -u jane@example.test mailbox Sent all || true; "
   "d=/var/mail/example.test/jane; mkdir -p $d/.Sent && tar -xf /tmp/demo.tar -C $d && rm /tmp/demo.tar "
   "&& chown -R docker:docker $d && doveadm force-resync -u jane@example.test '*'")
print(sh("doveadm mailbox status -u jane@example.test 'messages unseen' INBOX Sent").stdout.strip())
