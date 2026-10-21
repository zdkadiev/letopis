All keys and certificates here are **public test data**, not credentials.
The CA signs localhost (DNS localhost, IP 127.0.0.1) and wrong.invalid fixtures.
They are only used by loopback test servers and disposable-container trust tests.
Never install this CA in a host Keychain or host system certificate store.
Certificates were generated on 2026-10-21 with RSA 2048/SHA256, validity 3650 days.
The CA private key is included solely to make fixture renewal reproducible;
expiry of any fixture should fail the TLS tests and requires regenerating the PKI.
