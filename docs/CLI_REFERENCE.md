# CLI reference — actual development preview

Run `rit --help` and each subcommand's `--help` for flags. Common flags include `--config`, `--log-level`, `--log-format`, `--data-dir`, `--git-executable`, and reserved `--credential-file` (no authentication yet). Exit codes: 0 success, 2 usage, 3 configuration, 4 runtime/unavailable, 5 initialization.

Implemented: `doctor [--json]`, `config validate`, `inspect [PATH] [--json]`, `repository branches [PATH] [--json]`, `repository refs [PATH] [--json]`, `repository log [PATH] [--limit N] [--json]` (`repo` is alias). Direct Git inspection does not require or use a RIT registry; history is HEAD-only and omits commit messages. Output IDs include Git hash algorithm. Unicode-only paths/status are currently supported.

Experimental and unauthenticated: `init`, `repository register`, `repository list`, `repository info`, `status`, `storage verify`, `storage adopt-legacy`. They rely on incomplete registry security or legacy adoption. Do not use them with valuable data; adoption does not implement the required backup/intent protocol. Registry access is disabled on non-Linux platforms.

Explicitly unavailable: `identity {bootstrap,create-human,create-agent,list,info,suspend,reactivate,revoke,credential {issue,rotate,revoke}}`, `whoami`, `permission {roles,grant,revoke,list,check}`, `policy show`, `audit {list,verify}`, `workspace {create,list,status,remove}`, `agent {run,activity}`, `checkpoint {create,list,show,preview,restore}`, `submission {create,list,preview,publish}`, `task {create,list,status,review}`. They return a nonzero unavailable error without opening RIT storage. The broader v2 hierarchy is not implemented.
