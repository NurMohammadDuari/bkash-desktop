# Security Policy

## Supported versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | Yes       |

## Reporting a vulnerability

Email the maintainers privately. Do not open a public issue for an unfixed
vulnerability.

## Threat model in one paragraph

This project stores **no credentials**. It ships with an offline simulated
backend and, by default, makes no network calls at all. The `--live` backend
performs read-only gateway calls but refuses to create a session, because the
authentication handshake was never captured. The AES key handling wipes memory
on drop and never prints key material.

## What this project will not do

* Bypass device attestation, root detection, or emulator detection on any
  third-party app.
* Capture or store another person's banking credentials.
* Perform live financial transactions.
