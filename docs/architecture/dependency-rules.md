# Dependency rules

- Core must not depend on CLI, terminal presentation, global runtime initialization, database or Git internals.
- CLI may depend on core, not vice versa. Future application/domain services should depend on contracts; infrastructure adapters implement ports. Do not use circular crate dependencies.
- Interfaces call shared application services; never duplicate authorization or integration rules in clients.
- Add crates for real implemented functionality or immediately used boundaries, not future placeholders.
- For each external dependency record purpose, selected features, maintenance, MSRV, license and transitive cost; pin with Cargo.lock and review changes. No wildcard versions, custom cryptography or unnecessary async runtime.
- Keep `unsafe` forbidden in initial crates; later exceptions require explicit review and safety rationale.
