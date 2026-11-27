//! OpenPGP through the user's `gpg`: finding armored messages in post bodies, decrypting
//! them and encrypting drafts. Keys, agent and pinentry are gpg's; nothing is stored here.

use std::io::Write;
use std::process::{Command, Stdio};

const BEGIN: &str = "-----BEGIN PGP MESSAGE-----";
const END: &str = "-----END PGP MESSAGE-----";

/// A decrypted message and what gpg said about its signature, if it had one.
#[derive(Clone, Debug)]
pub struct Decrypted {
    pub text: String,
    pub signature: Option<String>,
}

#[derive(Debug)]
pub enum Error {
    /// The secret key needs a passphrase the agent does not have cached.
    NeedPassphrase,
    Other(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NeedPassphrase => write!(f, "passphrase needed"),
            Error::Other(s) => write!(f, "{s}"),
        }
    }
}

/// Byte range of the first armored message in `body`, widened to a code fence that
/// encloses it on its own lines.
pub fn find_block(body: &str) -> Option<(usize, usize)> {
    let b = body.find(BEGIN)?;
    let e = body[b..].find(END)? + b + END.len();
    let mut start = body[..b].rfind('\n').map_or(0, |i| i + 1);
    let mut end = body[e..].find('\n').map_or(body.len(), |i| e + i);
    // a fence line right before and after the block belongs to it
    let prev = body[..start.saturating_sub(1)]
        .rfind('\n')
        .map_or(0, |i| i + 1);
    let next_end = body[(end + 1).min(body.len())..]
        .find('\n')
        .map_or(body.len(), |i| end + 1 + i);
    let fence = |s: &str| s.trim().starts_with("```") || s.trim().starts_with("~~~");
    if start > 0 && end < body.len() && fence(&body[prev..start]) && fence(&body[end + 1..next_end])
    {
        start = prev;
        end = next_end;
    }
    Some((start, end))
}

/// The armored text of the block at `range`, lines unindented, as gpg wants it.
pub fn armor(body: &str, range: (usize, usize)) -> String {
    let block = &body[range.0..range.1];
    let from = block.find(BEGIN).unwrap_or(0);
    let to = block.find(END).map_or(block.len(), |i| i + END.len());
    let mut s: String = block[from..to]
        .lines()
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join("\n");
    s.push('\n');
    s
}

/// Recipients typed as a list separated by commas or whitespace.
pub fn recipients(s: &str) -> Vec<String> {
    s.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|r| !r.is_empty())
        .map(String::from)
        .collect()
}

struct Out {
    ok: bool,
    stdout: Vec<u8>,
    /// `[GNUPG:]` status lines, prefix removed.
    status: Vec<String>,
    /// gpg's own messages.
    messages: Vec<String>,
}

fn run(gpg: &str, args: &[&str], input: &str) -> Result<Out, Error> {
    let mut child = Command::new(gpg)
        .args(["--batch", "--no-tty", "--status-fd", "2"])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Other(format!("{gpg}: {e}")))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let input = input.to_string();
    // write on another thread so a full stdout pipe cannot block us
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let out = child
        .wait_with_output()
        .map_err(|e| Error::Other(format!("{gpg}: {e}")))?;
    let _ = writer.join();
    let (mut status, mut messages) = (vec![], vec![]);
    for l in String::from_utf8_lossy(&out.stderr).lines() {
        match l.strip_prefix("[GNUPG:] ") {
            Some(s) => status.push(s.to_string()),
            None if !l.trim().is_empty() => messages.push(l.trim().to_string()),
            None => {}
        }
    }
    Ok(Out {
        ok: out.status.success(),
        stdout: out.stdout,
        status,
        messages,
    })
}

/// Decrypt `armor`. With `ask` false, gpg may not prompt: a passphrase that is not
/// cached gives `NeedPassphrase`. With `ask` true pinentry may prompt (the caller has
/// handed the terminal over).
pub fn decrypt(gpg: &str, armor: &str, ask: bool) -> Result<Decrypted, Error> {
    let mut args = vec!["--decrypt"];
    if !ask {
        args.extend(["--pinentry-mode", "error"]);
    }
    let out = run(gpg, &args, armor)?;
    let decrypted = out.status.iter().any(|s| s == "DECRYPTION_OKAY");
    if !out.ok && !decrypted {
        return Err(decrypt_error(&out, ask));
    }
    Ok(Decrypted {
        text: String::from_utf8_lossy(&out.stdout).trim_end().to_string(),
        signature: signature(&out.status),
    })
}

fn decrypt_error(out: &Out, ask: bool) -> Error {
    // `ERROR pkdecrypt_failed <code>`: the low 16 bits are the gpg-error code
    let code = out.status.iter().find_map(|s| {
        let rest = s.strip_prefix("ERROR pkdecrypt_failed ")?;
        rest.trim().parse::<u32>().ok().map(|c| c & 0xffff)
    });
    match code {
        Some(85) if !ask => Error::NeedPassphrase,
        Some(85) => Error::Other("no pinentry (set GPG_TTY or use a GUI pinentry)".into()),
        Some(17) => Error::Other("no secret key for this message".into()),
        Some(11) => Error::Other("bad passphrase".into()),
        Some(99) => Error::Other("cancelled".into()),
        _ => Error::Other(
            out.messages
                .iter()
                .rev()
                .find(|m| m.starts_with("gpg:"))
                .cloned()
                .unwrap_or_else(|| "gpg failed".into()),
        ),
    }
}

/// What the status lines say about a signature, if there was one.
fn signature(status: &[String]) -> Option<String> {
    for s in status {
        let mut it = s.splitn(3, ' ');
        let (k, _id, rest) = (it.next()?, it.next().unwrap_or(""), it.next().unwrap_or(""));
        let r = match k {
            "GOODSIG" => format!("signed by {rest}"),
            "EXPSIG" => format!("expired signature by {rest}"),
            "EXPKEYSIG" => format!("signed by expired key of {rest}"),
            "REVKEYSIG" => format!("signed by revoked key of {rest}"),
            "BADSIG" => format!("BAD signature claiming {rest}"),
            "ERRSIG" => "signed, but the key is unknown".into(),
            _ => continue,
        };
        return Some(r);
    }
    None
}

/// Encrypt `text` to `to`, ASCII-armored. No passphrase is involved.
pub fn encrypt(gpg: &str, text: &str, to: &[String]) -> Result<String, Error> {
    if to.is_empty() {
        return Err(Error::Other("no recipients".into()));
    }
    let mut args = vec!["--armor", "--encrypt"];
    for r in to {
        args.extend(["--recipient", r.as_str()]);
    }
    let out = run(gpg, &args, text)?;
    if let Some(s) = out.status.iter().find(|s| s.starts_with("INV_RECP")) {
        let mut it = s.splitn(3, ' ').skip(1);
        let code = it.next().unwrap_or("");
        let who = it.next().unwrap_or("?");
        let why = match code {
            "1" => "no public key",
            "4" => "key revoked",
            "5" => "key expired",
            "10" => "key not trusted (sign or trust it in gpg)",
            _ => "unusable key",
        };
        return Err(Error::Other(format!("{who}: {why}")));
    }
    if !out.ok {
        let msg = out.messages.last().cloned().unwrap_or("gpg failed".into());
        return Err(Error::Other(msg));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MSG: &str = "-----BEGIN PGP MESSAGE-----\n\nhQEMA\n=abcd\n-----END PGP MESSAGE-----";

    #[test]
    fn finds_plain_block() {
        let body = format!("hi\n{MSG}\nbye");
        let r = find_block(&body).unwrap();
        assert_eq!(&body[r.0..r.1], MSG);
        assert_eq!(armor(&body, r), format!("{MSG}\n"));
    }

    #[test]
    fn widens_to_fence_and_unindents() {
        let body = format!("hi\n```\n  {}\n```\nbye", MSG.replace('\n', "\n  "));
        let r = find_block(&body).unwrap();
        assert!(body[r.0..r.1].starts_with("```"));
        assert!(body[r.0..r.1].ends_with("```"));
        assert_eq!(&body[r.1..], "\nbye");
        assert_eq!(armor(&body, r), format!("{MSG}\n"));
    }

    #[test]
    fn no_block_without_end() {
        assert!(find_block("-----BEGIN PGP MESSAGE-----\nxx").is_none());
        assert!(find_block("nothing").is_none());
    }

    #[test]
    fn classifies_errors() {
        let out = |code: &str| Out {
            ok: false,
            stdout: vec![],
            status: vec![format!("ERROR pkdecrypt_failed {code}")],
            messages: vec![],
        };
        assert!(matches!(
            decrypt_error(&out("67108949"), false),
            Error::NeedPassphrase
        ));
        assert!(matches!(
            decrypt_error(&out("67108949"), true),
            Error::Other(_)
        ));
        let e = decrypt_error(&out("33554449"), false).to_string();
        assert_eq!(e, "no secret key for this message");
    }

    #[test]
    fn parses_signature_and_recipients() {
        let st = vec!["DECRYPTION_OKAY".into(), "GOODSIG ABCD Alice <a@x>".into()];
        assert_eq!(signature(&st).unwrap(), "signed by Alice <a@x>");
        assert!(signature(&["DECRYPTION_OKAY".into()]).is_none());
        assert_eq!(recipients("a@x, bob  C0FFEE"), ["a@x", "bob", "C0FFEE"]);
    }
}
