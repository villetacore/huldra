//! Help and documentation for the system image: `help NAME` text for every
//! program, taken from the `//!` comment at the top of its source, and the
//! markdown documentation from `docs/`.
//!
//! Installed as /usr/share/huldra/help/NAME, an INDEX of
//! `name<TAB>synopsis<TAB>summary` lines, and /usr/share/huldra/docs/*.md.

use crate::image::ImageFile;
use crate::{root, target_dir, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// The leading `//!` block of a source file.
pub fn doc_comment(src: &str) -> Vec<String> {
    src.lines()
        .take_while(|l| l.starts_with("//!"))
        .map(|l| l.strip_prefix("//! ").unwrap_or(l.strip_prefix("//!").unwrap_or(l)).to_string())
        .collect()
}

/// (synopsis, summary) from a doc block: `name [args]: what it does.`
pub fn split_summary(doc: &[String]) -> (String, String) {
    let first_para: Vec<&str> = doc.iter().map(|s| s.trim()).take_while(|l| !l.is_empty()).collect();
    let para = first_para.join(" ");
    match para.split_once(": ") {
        Some((syn, sum)) if !syn.is_empty() => (syn.to_string(), sum.to_string()),
        _ => (para, String::new()),
    }
}

fn program_sources() -> Result<Vec<(String, PathBuf)>> {
    let dir = root().join("user").join("src").join("bin");
    let mut out = Vec::new();
    for e in fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = e.path();
        if path.join("main.rs").is_file() {
            out.push((name, path.join("main.rs")));
        } else if let Some(n) = name.strip_suffix(".rs") {
            out.push((n.to_string(), path));
        }
    }
    out.sort();
    Ok(out)
}

fn doc_files() -> Result<Vec<ImageFile>> {
    let mut files = Vec::new();
    let dir = root().join("docs");
    for e in fs::read_dir(&dir).map_err(|e| format!("docs/: {e}"))?.flatten() {
        let path = e.path();
        if path.extension().is_some_and(|x| x == "md") {
            let name = e.file_name().to_string_lossy().into_owned();
            files.push(ImageFile { dest: format!("usr/share/huldra/docs/{name}"), source: path, mode: 0o644 });
        }
    }
    files.push(ImageFile { dest: "usr/share/huldra/docs/overview.md".into(), source: root().join("README.md"), mode: 0o644 });
    files.sort_by(|a, b| a.dest.cmp(&b.dest));
    Ok(files)
}

/// Writes target/help and returns everything to install.
pub fn files() -> Result<Vec<ImageFile>> {
    let out = target_dir().join("help");
    if out.exists() {
        fs::remove_dir_all(&out).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let mut index = String::new();
    let mut files = Vec::new();
    for (name, src) in program_sources()? {
        let text = fs::read_to_string(&src).map_err(|e| format!("{}: {e}", src.display()))?;
        let doc = doc_comment(&text);
        if doc.is_empty() {
            return Err(format!("{}: no //! description (needed for `help {name}`)", rel(&src)));
        }
        let (synopsis, summary) = split_summary(&doc);
        index.push_str(&format!("{name}\t{synopsis}\t{summary}\n"));
        let file = out.join(&name);
        fs::write(&file, doc.join("\n") + "\n").map_err(|e| e.to_string())?;
        files.push(ImageFile { dest: format!("usr/share/huldra/help/{name}"), source: file, mode: 0o644 });
    }
    let idx = out.join("INDEX");
    fs::write(&idx, index).map_err(|e| e.to_string())?;
    files.push(ImageFile { dest: "usr/share/huldra/help/INDEX".into(), source: idx, mode: 0o644 });
    files.extend(doc_files()?);
    Ok(files)
}

fn rel(p: &Path) -> String {
    p.strip_prefix(root()).unwrap_or(p).display().to_string().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries() {
        let doc = doc_comment("//! cat [file...]: print files\n//! to stdout.\n//!\n//! More.\n\nfn main() {}\n");
        assert_eq!(doc, ["cat [file...]: print files", "to stdout.", "", "More."]);
        assert_eq!(split_summary(&doc), ("cat [file...]".into(), "print files to stdout.".into()));
        let doc = doc_comment("//! ls [-l] [path...]\n");
        assert_eq!(split_summary(&doc), ("ls [-l] [path...]".into(), String::new()));
    }

    #[test]
    fn every_program_has_a_description() {
        for (name, src) in program_sources().unwrap() {
            let doc = doc_comment(&fs::read_to_string(&src).unwrap());
            assert!(!doc.is_empty(), "{name} has no //! description");
            assert!(doc[0].starts_with(&name) || doc[0].starts_with(&format!("/bin/{name}")) || doc[0].starts_with(&format!("/sbin/{name}")), "{name}: description should start with the command: {:?}", doc[0]);
        }
    }
}
