use crate::value::Value;
use std::collections::HashMap;

struct Binding {
    value: Value,
    is_mut: bool,
}

pub enum AssignError {
    Undefined,
    Immutable,
}

/// A stack of scopes. The last one is the innermost
pub struct Env {
    scopes: Vec<HashMap<String, Binding>>,
}

impl Default for Env {
    fn default() -> Self {
        Self::new()
    }
}

impl Env {
    pub fn new() -> Self {
        Env {
            scopes: vec![HashMap::new()],
        }
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// `let`: creates a variable in the innermost scope
    /// If the name already exists in that same scope, its shadowed
    pub fn define(&mut self, name: &str, value: Value, is_mut: bool) {
        let scope = self.scopes.last_mut().expect("there is always a scope");
        scope.insert(name.to_string(), Binding { value, is_mut });
    }

    /// Search innermost to outermost
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .map(|b| &b.value)
    }

    /// `x = ...`: updates the nearest existing variable called `name`
    pub fn assign(&mut self, name: &str, value: Value) -> Result<(), AssignError> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(binding) = scope.get_mut(name) {
                if !binding.is_mut {
                    return Err(AssignError::Immutable);
                }
                binding.value = value;
                return Ok(());
            }
        }
        Err(AssignError::Undefined)
    }
}
