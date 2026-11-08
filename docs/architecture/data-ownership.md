# Data ownership

Git remains authoritative for source history, objects, index and refs; a remote Git server is authoritative for its own remote refs. Future RIT metadata may hold identities, sessions, coordination, policy, audit and references to Git OIDs but is not authoritative for Git contents. Objects retained by future `refs/rit/` may be visible to local users or broad push/mirror refspecs and may contain sensitive data. No Git objects, metadata database or private refs are created by this phase.
