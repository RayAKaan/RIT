# Deployment guide

No production release or installer exists. On a development machine install the pinned rustup toolchain and a C compiler, Git 2.29 or later, and run `cargo build --locked --workspace`. The resulting `rit` and `rit-protocol` executables are development previews. Windows and macOS storage backends are not implemented; native build or cross-compilation is not platform security validation. Do not deploy the registry for valuable data. No upgrade/migration, uninstall cleanup, package provenance or binary distribution guarantees are available.
