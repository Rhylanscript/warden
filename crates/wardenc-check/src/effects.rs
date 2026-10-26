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
