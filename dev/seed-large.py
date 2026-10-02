#!/usr/bin/env python3
"""Fills carol@example.test's INBOX with N generated messages (default 20000) for
performance work: written straight into her Maildir, then indexed by Dovecot.
  dev/seed-large.py [N]
"""
import os, random, subprocess, sys, tarfile, tempfile, time
from email.utils import formatdate

N = int(sys.argv[1]) if len(sys.argv) > 1 else 20000
HERE = os.path.dirname(os.path.abspath(__file__))
dc = ["docker", "compose", "-f", os.path.join(HERE, "compose.yml")]
sh = lambda cmd: subprocess.run(dc + ["exec", "-T", "mail", "sh", "-c", cmd], check=True, capture_output=True, text=True)

accounts = sh("setup email list || true").stdout
if "carol@example.test" not in accounts:
    sh("setup email add carol@example.test carolpass")
    time.sleep(3)
# Start from an empty INBOX so re-running replaces rather than piles up.
sh("doveadm expunge -u carol@example.test mailbox INBOX all || true")

random.seed(7)
names = ["Ana", "Bat", "Chen", "Dulma", "Erik", "Fatima", "Gan", "Hiro", "Ines", "Jamal"]
topics = ["Quarterly report", "Invoice", "Meeting notes", "Server alert", "Lunch", "Contract",
          "Release plan", "Customer feedback", "Holiday schedule", "Build failed"]
start = time.time() - 3 * 365 * 86400
roots = []
with tempfile.TemporaryDirectory() as tmp:
    cur = os.path.join(tmp, "cur")
    os.makedirs(cur)
    for i in range(N):
        t = start + i * (3 * 365 * 86400 / N)
        who = random.choice(names)
        mid = f"<seed-{i}@example.test>"
        refs = ""
        if roots and random.random() < 0.3:  # ~30% are replies in an existing thread
            root = random.choice(roots[-200:])
            refs = f"In-Reply-To: {root}\nReferences: {root}\n"
            subject = "Re: " + topics[hash(root) % len(topics)]
        else:
            roots.append(mid)
            subject = f"{random.choice(topics)} #{i}"
        body = (f"Hello Carol,\n\nThis is generated message {i} about {subject.lower()}.\n" * 20)
        msg = (f"From: {who} <{who.lower()}@example.test>\nTo: carol@example.test\n"
               f"Subject: {subject}\nDate: {formatdate(t)}\nMessage-ID: {mid}\n{refs}"
               f"MIME-Version: 1.0\nContent-Type: text/plain; charset=utf-8\n\n{body}")
        flags = "S" if random.random() < 0.85 else ""
        path = os.path.join(cur, f"{int(t)}.seed{i}.local:2,{flags}")
        with open(path, "w") as f:
            f.write(msg)
        os.utime(path, (t, t))  # Dovecot takes the arrival date from the file's mtime
    tar = os.path.join(tmp, "seed.tar")
    with tarfile.open(tar, "w", format=tarfile.PAX_FORMAT) as tf:
        tf.add(cur, arcname="cur")
    subprocess.run(dc + ["cp", tar, "mail:/tmp/seed.tar"], check=True, capture_output=True)
sh("d=/var/mail/example.test/carol; mkdir -p $d && tar -xf /tmp/seed.tar -C $d && rm /tmp/seed.tar "
   "&& chown -R docker:docker $d && doveadm force-resync -u carol@example.test INBOX "
   "&& doveadm index -u carol@example.test INBOX")
print(sh("doveadm mailbox status -u carol@example.test messages INBOX").stdout.strip())
