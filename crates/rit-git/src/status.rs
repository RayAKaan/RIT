use crate::process::GitError;
use rit_core::repository::{Change, Status};

fn text(bytes: &[u8]) -> Result<String, GitError> {
    String::from_utf8(bytes.to_vec()).map_err(|_| GitError::NonUnicode)
}
fn split_fields(record: &[u8], count: usize) -> Result<Vec<&[u8]>, GitError> {
    let mut parts = record.splitn(count, |b| *b == b' ').collect::<Vec<_>>();
    if parts.len() != count || parts.iter().any(|p| p.is_empty()) {
        return Err(GitError::Parse("status fields"));
    }
    Ok(std::mem::take(&mut parts))
}
/// Parse porcelain v2 -z, including the extra NUL record for renames.
pub(crate) fn parse(bytes: &[u8]) -> Result<Status, GitError> {
    if !bytes.is_empty() && !bytes.ends_with(&[0]) {
        return Err(GitError::Parse("status terminator"));
    }
    let mut status = Status {
        branch: None,
        oid: None,
        upstream: None,
        ahead: None,
        behind: None,
        changes: Vec::new(),
    };
    let mut records = bytes.split(|b| *b == 0).peekable();
    while let Some(r) = records.next() {
        if r.is_empty() {
            if records.peek().is_some() {
                return Err(GitError::Parse("empty status record"));
            }
            break;
        }
        if let Some(h) = r.strip_prefix(b"# ") {
            if let Some(v) = h.strip_prefix(b"branch.oid ") {
                status.oid = Some(text(v)?);
            } else if let Some(v) = h.strip_prefix(b"branch.head ") {
                status.branch = Some(text(v)?);
            } else if let Some(v) = h.strip_prefix(b"branch.upstream ") {
                status.upstream = Some(text(v)?);
            } else if let Some(v) = h.strip_prefix(b"branch.ab ") {
                let s = text(v)?;
                let mut i = s.split(' ');
                let a = i
                    .next()
                    .and_then(|v| v.strip_prefix('+'))
                    .and_then(|v| v.parse().ok())
                    .ok_or(GitError::Parse("ahead count"))?;
                let b = i
                    .next()
                    .and_then(|v| v.strip_prefix('-'))
                    .and_then(|v| v.parse().ok())
                    .ok_or(GitError::Parse("behind count"))?;
                if i.next().is_some() {
                    return Err(GitError::Parse("ahead/behind"));
                }
                status.ahead = Some(a);
                status.behind = Some(b);
            }
            continue;
        }
        let change = match r.first() {
            Some(b'?') => {
                let p = r
                    .strip_prefix(b"? ")
                    .ok_or(GitError::Parse("untracked path"))?;
                Change {
                    kind: "untracked".into(),
                    path: text(p)?,
                    original_path: None,
                    xy: None,
                    submodule: None,
                }
            }
            Some(b'1') | Some(b'2') | Some(b'u') => {
                let (kind, n) = match r[0] {
                    b'1' => ("ordinary", 9),
                    b'2' => ("renamed", 10),
                    _ => ("unmerged", 11),
                };
                let f = split_fields(r, n)?;
                if f[1].len() != 2 || f[2].len() != 4 {
                    return Err(GitError::Parse("status flags"));
                }
                let original_path = if r[0] == b'2' {
                    Some(text(
                        records
                            .next()
                            .filter(|p| !p.is_empty())
                            .ok_or(GitError::Parse("rename source"))?,
                    )?)
                } else {
                    None
                };
                Change {
                    kind: kind.into(),
                    path: text(f[n - 1])?,
                    original_path,
                    xy: Some(text(f[1])?),
                    submodule: Some(text(f[2])?),
                }
            }
            _ => return Err(GitError::Parse("unknown status record")),
        };
        status.changes.push(change);
    }
    Ok(status)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_nul_and_rename() {
        let b=b"# branch.oid (initial)\0# branch.head main\0# branch.ab +2 -3\x001 R. N... 100644 100644 100644 aa bb a b\x002 R. N... 100644 100644 100644 aa bb R100 new\nname\0old\tname\0? strange \"name\"\0";
        let s = parse(b).unwrap();
        assert_eq!(s.ahead, Some(2));
        assert_eq!(s.changes.len(), 3);
        assert_eq!(s.changes[1].original_path.as_deref(), Some("old\tname"));
        assert_eq!(s.changes[1].path, "new\nname");
    }
    #[test]
    fn rejects_unknown_and_truncated() {
        assert!(parse(b"x bad\0").is_err());
        assert!(parse(b"? missing terminator").is_err());
    }
}
