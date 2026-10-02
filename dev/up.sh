#!/usr/bin/env bash
# Start the dev mail server + Postgres and create two test mailboxes:
#   alice@example.test / alicepass      bob@example.test / bobpass
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p tls mail-config
if [[ ! -f tls/cert.pem ]]; then
  openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj "/CN=mail.example.test" \
    -addext "subjectAltName=DNS:mail.example.test,DNS:localhost,IP:127.0.0.1" \
    -keyout tls/key.pem -out tls/cert.pem 2>/dev/null
fi
# docker-mailserver only starts Dovecot if accounts exist at boot, so write them up front.
if [[ ! -s mail-config/postfix-accounts.cf ]]; then
  for acct in alice:alicepass bob:bobpass; do
    user=${acct%%:*} pass=${acct#*:}
    hash=$(docker run --rm --entrypoint doveadm ghcr.io/docker-mailserver/docker-mailserver:15 \
      pw -s SHA512-CRYPT -p "$pass")
    echo "$user@example.test|$hash" >> mail-config/postfix-accounts.cf
  done
fi
docker compose up -d
# Dovecot picks up new accounts within a few seconds; wait until IMAPS answers.
for i in $(seq 1 30); do
  (echo "a logout" | openssl s_client -quiet -connect localhost:3993 2>/dev/null | grep -q "OK") && break
  sleep 2
done
echo "mail: imaps localhost:3993, smtps localhost:3465   postgres: localhost:5434 (webmail/webmail)"
