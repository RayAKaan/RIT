# Troubleshooting

- **Operation unavailable:** the security gate or provider is not implemented; do not bypass it.
- **RecoveryBlocked / hot journal:** stop and preserve database and journal together. Do not delete, rename or manually replay sidecars. Follow the Phase 2.1 remediation report.
- **Database looks like RIT but may be replaced:** marker and schema do not establish file identity. Do not adopt or write it; the substitution test is failing.
- **Windows/macOS registry unavailable:** no native verified backend exists.
- **Git non-Unicode path rejected:** current adapter requires UTF-8 and fails rather than lossily converting.
- **MCP tool call denied:** the server deliberately exposes zero tools and does not authenticate actors.
