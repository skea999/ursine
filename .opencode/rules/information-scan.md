---
globs:
  - ".githooks/**"
  - ".opencode/rules/**"
keywords:
  - "pre-commit"
  - "commit"
  - "push"
  - "scan"
  - "PII"
  - "secret"
  - "personal"
  - "privacy"
---
- block: secrets+PII pre-commit
- scan: *.ts *.js *.py *.rb *.php *.java *.go *.rs *.json *.yaml *.yml *.env* *.md *.sh *.ps1 *.sql; skip node_modules .git dist build coverage *.min.js
- secrets: api[_-]?key|token|password|-----BEGIN PRIVATE KEY-----|AKIA[0-9A-Z]{16}|(mongodb|postgres|mysql|redis)://
- pii: email/name(allow git user); phone/address/city/provenance(block)
- audit: git log -p --all -S "api_key","password","secret","token"; leak: filter-branch+push+rotate
- always: scan block whitelist file:line history; never: skip allow-secrets hardcode commit-if-blocked
