#!/usr/bin/env python3
"""Seeds the dev mailbox with an HTML newsletter (remote images, CSS url(), a <script>,
a javascript: link) — the hostile-HTML case the e2e walkthrough checks is neutralised."""
import os
import smtplib
import ssl
from email.message import EmailMessage
from email.utils import formatdate

m = EmailMessage()
m["From"] = "Newsletter Team <bob@example.test>"
m["To"] = "alice@example.test"
m["Subject"] = "October product update 🚀"
m["Date"] = formatdate(localtime=True)
m.set_content("Plain fallback text")
m.add_alternative(
    """<html><head><style>.hero{background:url(https://tracker.example/bg.png);padding:24px}
h1{color:#3b5bdb}</style></head><body><div class="hero"><h1>What's new in October</h1>
<p>We shipped <b>faster search</b> and <i>dark mode</i>.</p>
<img src="https://tracker.example/pixel.gif" width="1" height="1">
<script>document.title="pwned"</script>
<p><a href="https://example.com/blog">Read the blog post</a> · <a href="javascript:alert(1)">bad link</a></p>
</div></body></html>""",
    subtype="html",
)
host = os.environ.get("SMTP_HOST", "localhost")
port = int(os.environ.get("SMTP_PORT", "3465"))
ctx = ssl._create_unverified_context()  # dev stack only: self-signed certificate
with smtplib.SMTP_SSL(host, port, context=ctx) as s:
    s.login("bob@example.test", "bobpass")
    s.send_message(m)
print("seeded: HTML newsletter for alice")
