// SPDX-License-Identifier: MIT OR Apache-2.0
//! Authored buffer names stay separate from their resolved filesystem targets.
use std::path::{Component, Path, PathBuf};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BufferReference {
    pub name: String,
    pub authored: Option<PathBuf>,
    pub resolved: Option<PathBuf>,
    pub diagnostic: Option<String>,
}
pub fn resolve(
    name: String,
    authored: Option<PathBuf>,
    owner: Option<&Path>,
    search: &[PathBuf],
) -> BufferReference {
    resolve_with(name, authored, owner, search, |p| p.is_file())
}
fn resolve_with(
    name: String,
    authored: Option<PathBuf>,
    owner: Option<&Path>,
    search: &[PathBuf],
    exists: impl Fn(&Path) -> bool,
) -> BufferReference {
    let candidates: Vec<PathBuf> = match &authored {
        None => vec![],
        Some(p) if p.is_absolute() => vec![p.clone()],
        Some(p) => owner
            .into_iter()
            .chain(search.iter().map(PathBuf::as_path))
            .map(|base| base.join(p))
            .collect(),
    };
    let resolved = candidates.into_iter().find(|p| exists(p));
    let diagnostic = if let Some(path) = &authored
        && resolved.is_none()
    {
        Some(format!(
            "Buffer {name}: unresolved reference {}",
            path.display()
        ))
    } else {
        None
    };
    BufferReference {
        name,
        authored,
        resolved,
        diagnostic,
    }
}
/// Both paths must be absolute; crossing filesystem roots uses an absolute reference.
pub fn relative_to(target: &Path, base: &Path) -> Option<PathBuf> {
    if !target.is_absolute() || !base.is_absolute() {
        return None;
    }
    let a: Vec<_> = target.components().collect();
    let b: Vec<_> = base.components().collect();
    if a.first() != b.first()
        || matches!(a.first(), Some(Component::Prefix(_))) && a.get(1) != b.get(1)
    {
        return None;
    }
    let common = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
    let mut result = PathBuf::new();
    for _ in common..b.len() {
        result.push("..");
    }
    for c in &a[common..] {
        result.push(c.as_os_str());
    }
    Some(result)
}
pub fn rebase(
    text: &str,
    references: &[BufferReference],
    directory: &Path,
) -> Result<(String, Vec<String>), String> {
    let mut value: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut warnings = vec![];
    if let Some(buffers) = value.get_mut("buffers").and_then(toml::Value::as_array_mut) {
        for buffer in buffers {
            let Some(reference) = references
                .iter()
                .find(|r| buffer.get("name").and_then(toml::Value::as_str) == Some(&r.name))
            else {
                continue;
            };
            match (&reference.authored, &reference.resolved) {
                (Some(authored), Some(target)) if authored.is_relative() => {
                    let path = relative_to(target, directory).unwrap_or_else(|| target.clone());
                    let path = path
                        .to_str()
                        .ok_or("Buffer path is not UTF-8; cannot represent it in .twf")?;
                    buffer["path"] = path.into();
                }
                (Some(_), None) => warnings.push(reference.diagnostic.clone().unwrap_or_default()),
                _ => {}
            }
        }
    }
    Ok((
        toml::to_string(&value).map_err(|e| e.to_string())?,
        warnings,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolution_precedence_builtins_and_missing() {
        let search = vec![PathBuf::from("/data/first"), PathBuf::from("/data/second")];
        let r = resolve_with(
            "sample".into(),
            Some("x.wav".into()),
            Some(Path::new("/flow")),
            &search,
            |_| true,
        );
        assert_eq!(r.resolved, Some("/flow/x.wav".into()));
        let r = resolve_with(
            "sample".into(),
            Some("x.wav".into()),
            Some(Path::new("/flow")),
            &search,
            |p| p.starts_with("/data"),
        );
        assert_eq!(r.resolved, Some("/data/first/x.wav".into()));
        let r = resolve_with(
            "sample".into(),
            Some("/absolute/x.wav".into()),
            Some(Path::new("/flow")),
            &search,
            |_| true,
        );
        assert_eq!(r.resolved, Some("/absolute/x.wav".into()));
        assert!(
            resolve_with("builtin".into(), None, None, &[], |_| false)
                .diagnostic
                .is_none()
        );
        assert!(
            resolve_with("missing".into(), Some("x.wav".into()), None, &[], |_| false)
                .diagnostic
                .is_some()
        );
    }
    #[test]
    fn save_as_preserves_target_and_unresolved_strings() {
        let refs = vec![
            BufferReference {
                name: "a".into(),
                authored: Some("sample.wav".into()),
                resolved: Some("/old/sample.wav".into()),
                diagnostic: None,
            },
            resolve_with("b".into(), Some("missing.wav".into()), None, &[], |_| false),
        ];
        let (text,warnings)=rebase("[flow]\nname='test'\n[[buffers]]\nname='a'\npath='sample.wav'\n[[buffers]]\nname='b'\npath='missing.wav'\n[[buffers]]\nname='builtin'\n",&refs,Path::new("/new")).unwrap();
        let v: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(v["buffers"][0]["path"].as_str(), Some("../old/sample.wav"));
        assert_eq!(v["buffers"][1]["path"].as_str(), Some("missing.wav"));
        assert!(v["buffers"][2].get("path").is_none());
        assert_eq!(warnings.len(), 1);
    }
}
