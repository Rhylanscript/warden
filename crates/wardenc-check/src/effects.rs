use std::collections::BTreeSet;

/// a dotted effect path like `io.print`, stored as its segments
pub type EffectPath = Vec<String>;

/// The effect an intrinsic function needs, if `name` is an intrinsic.
/// This is the one place that says which stdlib functions carry effects
pub fn intrinsic_effect(name: &str) -> Option<EffectPath> {
    match name {
        "print" => Some(vec!["io".to_string(), "print".to_string()]),
        _ => None,
    }
}

/// a parent covers its children: `io` covers `io.print` but not the other way round
pub fn covers(declared: &[String], required: &[String]) -> bool {
    required.starts_with(declared)
}

/// `["io", "print"]` --> `io.print`
pub fn display(path: &[String]) -> String {
    path.join(".")
}

/// A set of effects, (`{io.print, fs.read}`)
/// It keeps itself normalised: it never holds both parent and its own child
/// `{io, io.print}` collapses to `{io}` because `io` covers `io.print`
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectSet {
    paths: BTreeSet<EffectPath>,
}

impl EffectSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_paths(paths: impl IntoIterator<Item = EffectPath>) -> Self {
        let mut set = Self::new();
        for path in paths {
            set.insert(path);
        }
        set
    }

    /// Adds an effect, returns true if the set changed.
    /// The fixpoint loop uses this to know when to stop
    pub fn insert(&mut self, path: EffectPath) -> bool {
        if self.allows(&path) {
            return false;
        }
        self.paths.retain(|existing| !covers(&path, existing));
        self.paths.insert(path);
        true
    }

    /// Add everything from `other`, returns true if anything changed
    pub fn extend(&mut self, other: &EffectSet) -> bool {
        let mut changed = false;
        for path in &other.paths {
            changed |= self.insert(path.clone());
        }
        changed
    }

    /// Does this set permit `required`? True if some member covers it
    pub fn allows(&self, required: &[String]) -> bool {
        self.paths.iter().any(|have| covers(have, required))
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &EffectPath> {
        self.paths.iter()
    }

    /// `io.print, fs.read`, the text that goes after `uses`
    pub fn render(&self) -> String {
        self.paths
            .iter()
            .map(|p| display(p))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(s: &str) -> EffectPath {
        s.split('.').map(String::from).collect()
    }

    fn set(items: &[&str]) -> EffectSet {
        EffectSet::from_paths(items.iter().map(|s| path(s)))
    }

    #[test]
    fn insert_reports_whether_anything_changed() {
        let mut s = EffectSet::new();
        assert!(s.insert(path("io.print")));
        assert!(!s.insert(path("io.print")));
    }

    #[test]
    fn child_is_ignored_when_parent_is_present() {
        let mut s = set(&["io"]);
        assert!(!s.insert(path("io.print")));
        assert_eq!(s.render(), "io");
    }

    #[test]
    fn parent_replaces_existing_children() {
        let mut s = set(&["io.print", "io.read", "fs.read"]);
        assert!(s.insert(path("io")));
        assert_eq!(s.render(), "fs.read, io");
    }

    #[test]
    fn allows_follows_coverage() {
        let s = set(&["io"]);
        assert!(s.allows(&path("io.print")));
        assert!(!s.allows(&path("fs.read")));
        assert!(!set(&["io.print"]).allows(&path("io")));
    }

    #[test]
    fn extend_reports_change() {
        let mut a = set(&["io.print"]);
        assert!(a.extend(&set(&["fs.read"])));
        assert!(!a.extend(&set(&["fs.read", "io.print"])));
        assert_eq!(a.render(), "fs.read, io.print");
    }

    #[test]
    fn empty_set_is_pure() {
        let s = EffectSet::new();
        assert!(s.is_empty());
        assert_eq!(s.render(), "");
    }
}
