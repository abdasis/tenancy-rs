# Changelog

## 0.1.0

- Model tenant, domain, dan konfigurasi dengan validasi slug.
- Lifecycle task-local dengan bootstrapper, listener, event, dan guard.
- Resolver domain, subdomain, path, header, cookie, closure, dan rantai first-match.
- Repository memory, cached decorator, dan PostgreSQL.
- Port cache, storage, session, queue, dan enkripsi (memory, lokal, Redis, AES-256-GCM).
- Single-DB scoping, validasi unik/exists, dan RLS opt-in.
- Multi-DB registry pool, migrasi dan seed per tenant.
- Maintenance mode, impersonation token, create/update event, dan purge opt-in.
- Integrasi axum: layer tenant, extractor, router universal, respons 503.
- Helper testing, example axum, CI, dan buku mdBook.
