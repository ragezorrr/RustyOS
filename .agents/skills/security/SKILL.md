---
description: Use for IPC, clients, permissions, privileged helpers and
  untrusted input.
name: security
---

# Security

- Follow SECURITY.md.
- Apply least privilege and validate untrusted inputs.
- Check peer identity and permissions for local IPC where applicable.
- Avoid secrets in source, fixtures and logs.
- Minimize unsafe code and dependency surface.
- Do not bypass Polkit or system authorization.
- Escalate vulnerabilities clearly and safely.
